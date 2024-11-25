#![feature(iter_map_windows)]

use std::sync::Arc;

use alloy::{network::Network, providers::Provider, transports::Transport};
use alloy_primitives::{Address, BlockNumber};
use angstrom_types::{
    block_sync::BlockSyncConsumer,
    primitive::{PoolId, UniswapPoolRegistry}
};
use reth_provider::CanonStateNotifications;

use crate::uniswap::{
    pool::EnhancedUniswapPool, pool_data_loader::DataLoader, pool_manager::UniswapPoolManager,
    pool_providers::canonical_state_adapter::CanonicalStateAdapter
};

/// This module should have information on all the Constant Function Market
/// Makers that we work with.  Right now that's only Uniswap, but if there are
/// ever any others they will be added here
pub mod uniswap;

pub async fn configure_uniswap_manager<T: Transport + Clone, N: Network, S: BlockSyncConsumer>(
    provider: Arc<impl Provider<T, N>>,
    state_notification: CanonStateNotifications,
    uniswap_pool_registry: UniswapPoolRegistry,
    current_block: BlockNumber,
    block_sync: S,
    pool_manager: Address
) -> UniswapPoolManager<CanonicalStateAdapter, S, DataLoader<PoolId>, PoolId> {
    let mut uniswap_pools: Vec<_> = uniswap_pool_registry
        .pools()
        .keys()
        .map(|pool_id| {
            let initial_ticks_per_side = 200;
            EnhancedUniswapPool::new(
                DataLoader::new_with_registry(
                    *pool_id,
                    uniswap_pool_registry.clone(),
                    pool_manager
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

    let state_change_buffer = 100;
    UniswapPoolManager::new(
        uniswap_pools,
        current_block,
        state_change_buffer,
        Arc::new(CanonicalStateAdapter::new(state_notification)),
        block_sync
    )
}
