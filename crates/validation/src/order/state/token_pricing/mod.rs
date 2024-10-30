use std::{
    collections::{HashMap, VecDeque},
    pin::Pin,
    sync::Arc
};

use alloy::{
    primitives::{Address, FixedBytes, U256},
    providers::{Network, Provider},
    rpc::types::error,
    transports::Transport
};
use angstrom_types::{
    contract_payloads::rewards::PoolUpdate, pair_with_price::PairsWithPrice, primitive::PoolId
};
use futures::{Stream, StreamExt};
use matching_engine::cfmm::uniswap::{
    pool_data_loader::PoolDataLoader,
    pool_manager::{SyncedUniswapPools, UniswapPoolManager},
    pool_providers::PoolManagerProvider
};

const BLOCKS_TO_AVG_PRICE: usize = 5;
/// The token price generator gives us the avg instantaneous price of the last 5
/// blocks of the underlying V4 pool. This is then used in order to convert the
/// gas used from eth to token0 of the pool the user is swapping over.
/// In the case of NON direct eth pairs. we assume that any token liquid enough
/// to trade on angstrom not with eth will always have a eth pair 1 hop away.
/// this allows for a simple lookup.
pub struct TokenPriceGenerator {
    prev_prices:  HashMap<PoolId, VecDeque<PairsWithPrice>>,
    pair_to_pool: HashMap<(Address, Address), PoolId>,
    updates:      Pin<Box<dyn Stream<Item = Vec<PairsWithPrice>> + 'static>>,
    cur_block:    u64
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
        let mut pair_to_pool = HashMap::default();
        for (key, pool) in uni.iter() {
            let pool = pool.read().await;
            pair_to_pool.insert((pool.tokenA, pool.tokenB), key);
        }

        // for each pool, we want to load the last 5 blocks and get the sqrt_price_96
        // and then convert it into the price of the underlying pool
        let pools = futures::stream::iter(uni.iter())
            .map(|(pool_key, pool)| {
                let provider = provider.clone();

                async move {
                    let mut queue = VecDeque::new();
                    let pool_read = pool.read().await;

                    for block_number in current_block - BLOCKS_TO_AVG_PRICE..=current_block {
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

        Self { prev_prices: pools, updates, cur_block: current_block, pair_to_pool }
    }

    pub fn get_eth_conversion_price(&self, id: &PoolId) -> Option<U256> {
        let prices = self.prev_prices.get(id)?;
        let size = prices.len();
        if size != BLOCKS_TO_AVG_PRICE {
            warn!("size of loaded blocks doesn't match the value we set");
        }

        Some(
            prices
                .iter()
                .map(|price| price.price_1_over_0)
                .sum::<U256>()
                / U256::from(size)
        )
    }
}

impl Stream for TokenPriceGenerator {
    type Item = ();

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>
    ) -> std::task::Poll<Option<Self::Item>> {
        while let Poll::Ready(update) = self.updates.poll_next_unpin(cx) {
            let Some(update) = update else { return Poll::Ready(None) };

            for pool_update in update {
                let pool_update: PairsWithPrice = pool_update;
                // make sure we aren't replaying
                assert!(pool_update.block_num == self.cur_block + 1);

                let pool_key = self
                    .pair_to_pool
                    .get(&(pool_update.token0, pool_update.token1))
                    .expect("got pool update that we don't have stored");
                let prev_prices = self
                    .prev_prices
                    .get_mut(pool_key)
                    .expect("don't have prev_prices for update");
                prev_prices.pop_front();
                prev_prices.push_back(pool_update);
            }
            self.cur_block += 1;
        }

        Poll::Pending
    }
}
