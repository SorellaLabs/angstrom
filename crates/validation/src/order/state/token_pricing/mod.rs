use std::{
    collections::{HashMap, VecDeque},
    pin::Pin,
    sync::Arc
};

use alloy::{
    primitives::{Address, U256},
    providers::{Network, Provider},
    transports::Transport
};
use angstrom_types::{pair_with_price::PairsWithPrice, primitive::PoolId};
use futures::Stream;
use matching_engine::cfmm::uniswap::{
    pool_data_loader::PoolDataLoader, pool_manager::UniswapPoolManager,
    pool_providers::PoolManagerProvider
};

/// The token price generator gives us the avg instantaneous price of the last 5
/// blocks of the underlying V4 pool. This is then used in order to convert the
/// gas used from eth to token0 of the pool the user is swapping over.
/// In the case of NON direct eth pairs. we assume that any token liquid enough
/// to trade on angstrom not with eth will always have a eth pair 1 hop away.
/// this allows for a simple lookup.
pub struct TokenPriceGenerator<Pro, Loader: PoolDataLoader<Address>> {
    /// stores the last N amount of prices. TODO: (Address, Address) -> PoolKey
    /// once plamen updates.
    prev_prices: HashMap<PoolId, VecDeque<PairsWithPrice>>,
    updates:     Pin<Box<dyn Stream<Item = Vec<PairsWithPrice>> + 'static>>,
    uni:         Arc<UniswapPoolManager<Pro, Loader>>
}

impl<Pro, Loader: PoolDataLoader<Address>> TokenPriceGenerator<Pro, Loader>
where
    Loader: PoolDataLoader<Address> + Default + Clone + Send + Sync + 'static,
    Pro: PoolManagerProvider + Send + Sync + 'static
{
    /// is a bit of a pain as we need todo a look-back in-order to grab last 5
    /// blocks.
    pub async fn new<P: Provider<T, N>, T: Transport + Clone, N: Network>(
        provider: Arc<P>,
        current_block: u64,
        active_pairs: Vec<PoolId>,
        loader: Loader,
        uni: Arc<UniswapPoolManager<Pro, Loader>>
    ) -> eyre::Result<Self> {
        // for each pool, we want to load the last 5 blocks and get the sqrt_price_96
        // and then convert it into the price of the underlying pool

        for block_number in current_block - 5..=current_block {
            let pools = uni.pool_addresses().map(|pool_address| async {
                let pool_data = loader
                    .load_pool_data(Some(block_number), provider.clone())
                    .await
                    .expect("failed to load historical price for token price conversion");
                let price = pool_data.get_raw_price();

                PairsWithPrice {
                    token0:         pool_data.tokenA,
                    token1:         pool_data.tokenB,
                    block_num:      block_number,
                    price_1_over_0: price
                }
            });
        }

        todo!()
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
