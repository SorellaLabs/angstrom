use std::{
    collections::{HashMap, VecDeque},
    pin::Pin,
    sync::Arc
};

use alloy::{
    primitives::{Address, FixedBytes, U256},
    providers::{Network, Provider},
    transports::Transport
};
use angstrom_types::{pair_with_price::PairsWithPrice, primitive::PoolId};
use futures::{Stream, StreamExt};
use matching_engine::cfmm::uniswap::{
    pool_data_loader::PoolDataLoader,
    pool_manager::{SyncedUniswapPools, UniswapPoolManager},
    pool_providers::PoolManagerProvider
};

/// The token price generator gives us the avg instantaneous price of the last 5
/// blocks of the underlying V4 pool. This is then used in order to convert the
/// gas used from eth to token0 of the pool the user is swapping over.
/// In the case of NON direct eth pairs. we assume that any token liquid enough
/// to trade on angstrom not with eth will always have a eth pair 1 hop away.
/// this allows for a simple lookup.
pub struct TokenPriceGenerator {
    prev_prices: HashMap<PoolId, VecDeque<PairsWithPrice>>,
    updates:     Pin<Box<dyn Stream<Item = Vec<PairsWithPrice>> + 'static>>
}

impl TokenPriceGenerator {
    /// is a bit of a pain as we need todo a look-back in-order to grab last 5
    /// blocks.
    pub async fn new<P: Provider<T, N>, T: Transport + Clone, N: Network, Loader>(
        provider: Arc<P>,
        current_block: u64,
        updates: Pin<Box<dyn Stream<Item = Vec<PairsWithPrice>> + 'static>>,
        uni: SyncedUniswapPools<PoolId, Loader>
    ) -> eyre::Result<Self>
    where
        Loader: PoolDataLoader<PoolId> + Default + Clone + Send + Sync + 'static
    {
        // for each pool, we want to load the last 5 blocks and get the sqrt_price_96
        // and then convert it into the price of the underlying pool
        let pools = futures::stream::iter(uni.iter())
            .map(|(pool_key, pool)| {
                let provider = provider.clone();
                async move {
                    let mut queue = VecDeque::new();
                    for block_number in current_block - 5..=current_block {
                        let pool_read = pool.read().await;
                        let pool_data = pool_read
                            .pool_data_for_block(block_number, provider)
                            .await
                            .expect("failed to load historical price for token price conversion");
                        let price = pool_data.get_raw_price();

                        queue.push_back(PairsWithPrice {
                            token0:         pool_data.tokenA,
                            token1:         pool_data.tokenB,
                            block_num:      block_number,
                            price_1_over_0: price
                        });
                    }
                    (*pool_key, queue)
                }
            })
            .collect::<HashMap<_, _>>()
            .await;

        Self { prev_prices: pools, updates }
    }

    /// NOTE: assumes that the uniswap pool state transition has already
    /// occurred.
    pub fn on_new_block(&mut self) {}

    pub fn get_eth_conversion_price(&self, mut token0: Address, mut token1: Address) -> U256 {
        // sort tokens
        if token0 > token1 {
            std::mem::swap(&mut token0, &mut token1);
        }

        todo!()
    }
}
