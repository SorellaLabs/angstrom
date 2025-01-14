use std::{
    collections::{HashMap, HashSet},
    sync::Arc
};

use alloy::providers::Provider;
use alloy_primitives::{Address, BlockNumber};
use angstrom_types::{
    block_sync::BlockSyncConsumer,
    contract_payloads::angstrom::BundleGasDetails,
    matching::uniswap::PoolSnapshot,
    orders::PoolSolution,
    primitive::{PoolId, UniswapPoolRegistry},
    sol_bindings::{
        grouped_orders::OrderWithStorageData, rpc_orders::TopOfBlockOrder, RawPoolOrder
    }
};
use book::{BookOrder, OrderBook};
use futures_util::future::BoxFuture;
use reth_provider::CanonStateNotifications;
use uniswap_v4::uniswap::{
    pool::EnhancedUniswapPool, pool_data_loader::DataLoader, pool_manager::UniswapPoolManager,
    pool_providers::canonical_state_adapter::CanonicalStateAdapter
};

pub mod book;
pub mod manager;
pub mod matcher;
pub mod simulation;
pub mod strategy;

pub use manager::MatchingManager;

pub trait MatchingEngineHandle: Send + Sync + Clone + Unpin + 'static {
    fn solve_pools(
        &self,
        limit: Vec<BookOrder>,
        searcher: Vec<OrderWithStorageData<TopOfBlockOrder>>,
        pools: HashMap<PoolId, (Address, Address, PoolSnapshot, u16)>
    ) -> BoxFuture<eyre::Result<(Vec<PoolSolution>, BundleGasDetails)>>;
}

pub fn build_book(id: PoolId, amm: Option<PoolSnapshot>, orders: HashSet<BookOrder>) -> OrderBook {
    let (mut bids, mut asks): (Vec<BookOrder>, Vec<BookOrder>) =
        orders.into_iter().partition(|o| o.is_bid);

    // assert bids decreasing and asks increasing
    bids.sort_by_key(|b| std::cmp::Reverse(b.limit_price()));
    asks.sort_by_key(|a| a.limit_price());

    OrderBook::new(id, amm, bids, asks, Some(book::sort::SortStrategy::ByPriceByVolume))
}

pub async fn configure_uniswap_manager<BlockSync: BlockSyncConsumer>(
    provider: Arc<impl Provider + 'static>,
    state_notification: CanonStateNotifications,
    uniswap_pool_registry: UniswapPoolRegistry,
    current_block: BlockNumber,
    block_sync: BlockSync,
    pool_manager_address: Address
) -> UniswapPoolManager<
    CanonicalStateAdapter<impl Provider + 'static>,
    BlockSync,
    DataLoader<PoolId>,
    PoolId
> {
    let mut uniswap_pools: Vec<_> = uniswap_pool_registry
        .pools()
        .keys()
        .map(|pool_id| {
            let initial_ticks_per_side = 200;
            EnhancedUniswapPool::new(
                DataLoader::new_with_registry(
                    *pool_id,
                    uniswap_pool_registry.clone(),
                    pool_manager_address
                ),
                initial_ticks_per_side
            )
        })
        .collect();

    for pool in uniswap_pools.iter_mut() {
        pool.initialize(Some(current_block), provider.clone())
            .await
            .unwrap();
    }

    let notifier =
        Arc::new(CanonicalStateAdapter::new(state_notification, provider.clone(), current_block));

    UniswapPoolManager::new(uniswap_pools, current_block, notifier, block_sync)
}
