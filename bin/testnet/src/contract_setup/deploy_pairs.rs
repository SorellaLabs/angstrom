use std::{future::IntoFuture, pin::pin, time::Duration};

use alloy_primitives::{Address, I256, U256};
use alloy_provider::ext::AnvilApi;
use angstrom_types::sol_bindings::testnet::{MockERC20, TestnetHub};
use futures::Future;
use rand::{thread_rng, Rng};
use rand_distr::Distribution;
use tokio::time::timeout;

use crate::anvil_utils::AnvilWalletRpc;

pub struct AngstromTokens {
    pub token0:          Address,
    pub token1:          Address,
    pub init_sqrt_price: U256
}

pub async fn deploy_tokens_for_pairs(
    provider: AnvilWalletRpc,
    testnet_address: Address,
    pair_count: usize
) -> eyre::Result<Vec<AngstromTokens>> {
    let mut res = vec![];
    let mut rng = thread_rng();

    for _ in 0..pair_count {
        // if we don't do these sequentially, the provider nonce messes up and doesn't
        // deploy properly
        let token0 = anvil_mine_delay(
            Box::pin(MockERC20::deploy(provider.clone())),
            &provider,
            Duration::from_millis(100)
        )
        .await?;
        let token1 = anvil_mine_delay(
            Box::pin(MockERC20::deploy(provider.clone())),
            &provider,
            Duration::from_millis(100)
        )
        .await?;

        let token0 = *token0.address();
        let token1 = *token1.address();
        res.push(AngstromTokens {
            token0,
            token1,
            init_sqrt_price: U256::from_be_bytes([0, 0, rng.gen(), rng.gen()])
        })
    }

    // generates the pairs
    generate_pairs_with_liq(provider, testnet_address, &res).await?;

    Ok(res)
}

pub async fn generate_pairs_with_liq(
    provider: AnvilWalletRpc,
    testnet_address: Address,
    pairs: &[AngstromTokens]
) -> eyre::Result<()> {
    let hub = TestnetHub::new(testnet_address, provider.clone());
    for pair in pairs {
        // deploy the pair with a initial sqrt price
        anvil_mine_delay(
            Box::pin(
                hub.clone()
                    .initializePool(pair.token0, pair.token1, pair.init_sqrt_price)
                    .send()
                    .await?
                    .watch()
            ),
            &provider,
            Duration::from_millis(100)
        )
        .await?;

        let tick_spacing = hub
            .clone()
            .tick_spacing(pair.token0, pair.token1)
            .call()
            .await
            .unwrap()
            ._0;

        let current_tick = hub
            .clone()
            .fetch_current_tick(pair.token0, pair.token1)
            .call()
            .await
            .unwrap()
            ._0;

        let rng = thread_rng();

        // add a normal distro of liquidity around the initial sqrt price
        // arbitrary values for std. might want to alter later
        let distro = rand_distr::Normal::new(
            ((pair.init_sqrt_price).try_into() as Result<u128, _>)? as f64,
            ((pair.init_sqrt_price / U256::from(1000)).try_into() as Result<u128, _>)? as f64
        )?;

        for (tick_offset, liquidity) in distro.sample_iter(rng).take(100).enumerate() {
            // -50 + 50 ticks around the current tick
            let tick_offset = (tick_offset as i32 - 50) * tick_spacing;
            let tick_lower = current_tick + tick_offset;
            let tick_upper = tick_lower + tick_spacing;

            // add liquidity from rand distro
            anvil_mine_delay(
                Box::pin(
                    hub.modifyLiquidity(
                        pair.token0,
                        pair.token1,
                        tick_lower,
                        tick_upper,
                        I256::unchecked_from(liquidity as u128)
                    )
                    .send()
                    .await?
                    .watch()
                ),
                &provider,
                Duration::from_millis(10)
            )
            .await?;
        }
    }

    Ok(())
}

// will wait for a specific delay and then call anvil mine wallet.
// this will allow us to quick mine and not have to wait the 12 seconds
// between transactions while avoiding race conditions
pub async fn anvil_mine_delay<F0: Future + Unpin>(
    f0: F0,
    provider: &AnvilWalletRpc,
    delay: Duration
) -> F0::Output {
    let mut pinned = pin!(f0);
    if let Ok(v) = timeout(delay, &mut pinned).await {
        return v
    }
    provider
        .anvil_mine(Some(U256::from(1)), None)
        .await
        .expect("anvil failed to mine");

    pinned.await
}
