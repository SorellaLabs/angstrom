use alloy::{
    primitives::{
        aliases::{I24, U24},
        keccak256, Bytes
    },
    sol_types::SolValue
};
use angstrom_types::{
    contract_payloads::{
        rewards::{MockContractMessage, PoolUpdate, RewardsUpdate},
        Asset, Pair
    },
    matching::{
        uniswap::{LiqRange, PoolSnapshot},
        SqrtPriceX96
    }
};
use pade::PadeEncode;
use testing_tools::{
    contracts::{environment::mockreward::MockRewardEnv, DebugTransaction},
    providers::AnvilProvider
};
use uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick;

#[tokio::test]
async fn properly_communicates_tob_to_contract() -> eyre::Result<()> {
    let anvil = AnvilProvider::spawn_new_isolated().await.unwrap();
    let env = MockRewardEnv::with_anvil(anvil.wallet_provider())
        .await
        .unwrap();

    println!("Env created");
    let sqrt_price_x96 = SqrtPriceX96::from(get_sqrt_ratio_at_tick(100020).unwrap());
    let tick_spacing = I24::unchecked_from(60);
    let pool_fee = U24::ZERO;
    let snapshot = PoolSnapshot::new(
        vec![LiqRange::new(99900, 100140, 5_000_000_000_000_000_000_000_u128).unwrap()],
        sqrt_price_x96
    )?;
    let pool_key = env
        .create_pool_and_tokens_from_snapshot(tick_spacing, pool_fee, snapshot)
        .await
        .unwrap();

    let message = MockContractMessage {
        assets: vec![
            Asset { addr: pool_key.currency0, borrow: 0, save: 0, settle: 0 },
            Asset { addr: pool_key.currency1, borrow: 0, save: 0, settle: 0 },
        ],
        pairs:  vec![Pair {
            index0:       0,
            index1:       1,
            store_index:  0,
            price_1over0: sqrt_price_x96.into()
        }],
        update: PoolUpdate {
            zero_for_one:     false,
            pair_index:       0,
            swap_in_quantity: 0,
            rewards_update:   RewardsUpdate::MultiTick {
                start_tick:      I24::unchecked_from(100080_i32),
                start_liquidity: 5_000_000_000_000_000_000_000_u128,
                quantities:      vec![100000000_u128, 200000000_u128]
            }
        }
    };
    env.mock_reward()
        .update(Bytes::from(message.pade_encode()))
        .run_safe()
        .await
        .unwrap();
    let range_check_res = env
        .mock_reward()
        .getGrowthInsideRange(
            keccak256(pool_key.abi_encode()),
            I24::unchecked_from(99900),
            I24::unchecked_from(100140)
        )
        .gas(50e6 as u64)
        .call()
        .await
        .unwrap()
        ._0;
    println!("Got range check res: {}", range_check_res);
    Ok(())
}
