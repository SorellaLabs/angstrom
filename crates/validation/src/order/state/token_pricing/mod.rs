use std::{
    collections::{HashMap, VecDeque},
    sync::Arc
};

use alloy::primitives::{Address, FixedBytes, U256};
use angstrom_types::{pair_with_price::PairsWithPrice, primitive::PoolId};
use matching_engine::cfmm::uniswap::{
    pool_data_loader::PoolDataLoader, pool_manager::UniswapPoolManager
};

use crate::order::state::pools::angstrom_pools::AngstromPools;

/// The token price generator gives us the avg instantaneous price of the last 5
/// blocks of the underlying V4 pool. This is then used in order to convert the
/// gas used from eth to token0 of the pool the user is swapping over.
/// In the case of NON direct eth pairs. we assume that any token liquid enough
/// to trade on angstrom not with eth will always have a eth pair 1 hop away.
/// this allows for a simple lookup.
pub struct TokenPriceGenerator<Provider, Loader: PoolDataLoader<Address>> {
    /// stores the last N amount of prices. TODO: (Address, Address) -> PoolKey
    /// once plamen updates.
    prev_prices: HashMap<PoolId, VecDeque<PairsWithPrice>>,
    uni:         Arc<UniswapPoolManager<Provider, Loader>>
}

impl<Provider, Loader: PoolDataLoader<Address>> TokenPriceGenerator<Provider, Loader> {
    /// is a bit of a pain as we need todo a look-back in-order to grab last 5
    /// blocks.
    pub async fn new(
        current_block: u64,
        active_pairs: Vec<PoolId>,
        uni: &UniswapPoolManager<Provider, Loader>
    ) -> eyre::Result<Self> {
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
