use alloy::{contract::RawCallBuilder, primitives::Address, sol_types::SolValue};
use angstrom_types::contract_bindings::mock_rewards_manager::MockRewardsManager;

use super::{mine_address, uniswap_flags::UniswapFlags, DEFAULT_CREATE2_FACTORY};

pub async fn deploy_mock_rewards_manager<
    T: alloy::contract::private::Transport + ::core::clone::Clone,
    P: alloy::contract::private::Provider<T, N>,
    N: alloy::contract::private::Network
>(
    provider: &P,
    pool_manager: Address,
    controller: Address
) -> Address {
    deploy_mock_rewards_manager_with_factory(
        provider,
        pool_manager,
        DEFAULT_CREATE2_FACTORY,
        controller
    )
    .await
}

pub async fn deploy_mock_rewards_manager_with_factory<
    T: alloy::contract::private::Transport + ::core::clone::Clone,
    P: alloy::contract::private::Provider<T, N>,
    N: alloy::contract::private::Network
>(
    provider: &P,
    pool_manager: Address,
    factory: Address,
    controller: Address
) -> Address {
    let flags = UniswapFlags::BeforeSwap
        | UniswapFlags::BeforeInitialize
        | UniswapFlags::BeforeAddLiquidity
        | UniswapFlags::BeforeRemoveLiquidity;

    let mock_builder = MockRewardsManager::deploy_builder(&provider, pool_manager, controller);
    let (mock_tob_address, salt) =
        mine_address(flags, UniswapFlags::mask(), mock_builder.calldata());
    let final_mock_initcode = [salt.abi_encode(), mock_builder.calldata().to_vec()].concat();
    RawCallBuilder::new_raw(&provider, final_mock_initcode.into())
        .to(factory)
        .gas(10e6 as u64)
        .send()
        .await
        .unwrap()
        .watch()
        .await
        .unwrap();
    mock_tob_address
}
