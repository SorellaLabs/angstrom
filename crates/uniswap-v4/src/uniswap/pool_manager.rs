use std::{
    collections::HashMap,
    fmt::Debug,
    hash::Hash,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLock, RwLockReadGuard
    }
};

use alloy::{
    primitives::{Address, BlockNumber},
    rpc::types::{eth::Filter, Block},
    transports::{RpcError, TransportErrorKind}
};
use alloy_primitives::Log;
use angstrom_types::{matching::uniswap::PoolSnapshot, primitive::PoolId};
use arraydeque::ArrayDeque;
use futures_util::{stream::BoxStream, StreamExt};
use thiserror::Error;
use tokio::{
    sync::mpsc::{Receiver, Sender},
    task::JoinHandle
};

use super::pool::PoolError;
use crate::uniswap::{
    pool::EnhancedUniswapPool,
    pool_data_loader::{DataLoader, PoolDataLoader},
    pool_providers::PoolManagerProvider
};

pub type StateChangeCache<Loader, A> = HashMap<A, ArrayDeque<StateChange<Loader, A>, 150>>;
pub type SyncedUniswapPools<A = PoolId, Loader = DataLoader<A>> =
    Arc<HashMap<A, RwLock<EnhancedUniswapPool<Loader, A>>>>;

#[derive(Default)]
pub struct UniswapPoolManager<P, Loader: PoolDataLoader<A>, A = Address>
where
    A: Debug + Copy
{
    pools:               SyncedUniswapPools<A, Loader>,
    latest_synced_block: u64,
    state_change_buffer: usize,
    state_change_cache:  Arc<RwLock<StateChangeCache<Loader, A>>>,
    provider:            Arc<P>,
    sync_started:        AtomicBool
}

impl<P, Loader, A> UniswapPoolManager<P, Loader, A>
where
    A: Eq + Hash + Debug + Default + Copy + Sync + Send + 'static,
    Loader: PoolDataLoader<A> + Default + Clone + Send + Sync + 'static,
    P: PoolManagerProvider + Send + Sync + 'static
{
    pub fn new(
        pools: Vec<EnhancedUniswapPool<Loader, A>>,
        latest_synced_block: BlockNumber,
        state_change_buffer: usize,
        provider: Arc<P>
    ) -> Self {
        let rwlock_pools = pools
            .into_iter()
            .map(|pool| (pool.address(), RwLock::new(pool)))
            .collect();
        Self {
            pools: Arc::new(rwlock_pools),
            latest_synced_block,
            state_change_buffer,
            state_change_cache: Arc::new(RwLock::new(HashMap::new())),
            provider,
            sync_started: AtomicBool::new(false)
        }
    }

    pub fn fetch_pool_snapshots(&self) -> HashMap<A, PoolSnapshot> {
        self.pools
            .iter()
            .filter_map(|(key, pool)| {
                Some((*key, pool.read().unwrap().fetch_pool_snapshot().ok()?.2))
            })
            .collect()
    }

    pub fn pool_addresses(&self) -> impl Iterator<Item = &A> + '_ {
        self.pools.keys()
    }

    pub fn pools(&self) -> Arc<HashMap<A, RwLock<EnhancedUniswapPool<Loader, A>>>> {
        self.pools.clone()
    }

    pub fn pool(&self, address: &A) -> Option<RwLockReadGuard<'_, EnhancedUniswapPool<Loader, A>>> {
        let pool = self.pools.get(address)?;
        Some(pool.read().unwrap())
    }

    pub fn filter(&self) -> Filter {
        // it should crash given that no pools makes no sense
        let pool = self.pools.values().next().unwrap();
        let pool = pool.read().unwrap();
        Filter::new().event_signature(pool.event_signatures())
    }

    /// Listens to new blocks and handles state changes, sending the pool
    /// address if it incurred a state change in the block.
    pub async fn subscribe_state_changes(
        &self
    ) -> Result<
        (Receiver<(A, BlockNumber)>, JoinHandle<Result<(), PoolManagerError>>),
        PoolManagerError
    > {
        if self
            .sync_started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(PoolManagerError::SyncAlreadyStarted)
        }

        let (pool_updated_tx, pool_updated_rx) =
            tokio::sync::mpsc::channel::<(A, BlockNumber)>(self.state_change_buffer);

        let updated_pool_handle = self.handle_state_changes(Some(pool_updated_tx)).await?;

        Ok((pool_updated_rx, updated_pool_handle))
    }

    /// Listens to new blocks and handles state changes
    pub async fn watch_state_changes(
        &self
    ) -> Result<JoinHandle<Result<(), PoolManagerError>>, PoolManagerError> {
        if self
            .sync_started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(PoolManagerError::SyncAlreadyStarted)
        }

        let updated_pool_handle = self.handle_state_changes(None).await?;

        Ok(updated_pool_handle)
    }

    async fn handle_state_changes(
        &self,
        pool_updated_tx: Option<Sender<(A, BlockNumber)>>
    ) -> Result<JoinHandle<Result<(), PoolManagerError>>, PoolManagerError> {
        let mut last_synced_block = self.latest_synced_block;

        let pools = self.pools.clone();
        let provider = Arc::clone(&self.provider);
        let filter = self.filter();
        let state_change_cache = Arc::clone(&self.state_change_cache);
        let updated_pool_handle = tokio::spawn(async move {
            let mut block_stream: BoxStream<Option<u64>> = provider.subscribe_blocks();
            while let Some(block_number) = block_stream.next().await {
                let chain_head_block_number =
                    block_number.ok_or(PoolManagerError::EmptyBlockNumberFromStream)?;
                // If there is a reorg, unwind state changes from last_synced block to the
                // chain head block number
                let mut reorg = false;
                if chain_head_block_number <= last_synced_block {
                    tracing::trace!(
                        chain_head_block_number,
                        last_synced_block,
                        "reorg detected, unwinding state changes"
                    );
                    reorg = true;
                    // set the last synced block to the head block number
                    last_synced_block = chain_head_block_number - 1;
                }

                let logs = provider
                    .get_logs(
                        &filter
                            .clone()
                            // last_synced_block + 1 == chain_head_block_number (always)
                            .from_block(last_synced_block + 1)
                            .to_block(chain_head_block_number)
                    )
                    .await?;

                if reorg {
                    // scope for locks
                    let mut state_change_cache = state_change_cache.write().unwrap();
                    for pool in pools.values() {
                        let mut pool_guard = pool.write().unwrap();
                        Self::unwind_state_changes(
                            &mut pool_guard,
                            &mut state_change_cache,
                            chain_head_block_number
                        )?;
                    }
                }

                let logs_by_address = Loader::group_logs(logs);

                for (addr, logs) in logs_by_address {
                    if logs.is_empty() {
                        continue
                    }

                    let Some(pool) = pools.get(&addr) else {
                        continue;
                    };

                    // scope for locks
                    let address = {
                        let mut pool_guard = pool.write().unwrap();
                        let mut state_change_cache = state_change_cache.write().unwrap();
                        Self::handle_state_changes_from_logs(
                            &mut pool_guard,
                            &mut state_change_cache,
                            logs,
                            chain_head_block_number
                        )?;
                        pool_guard.address()
                    };

                    if let Some(tx) = &pool_updated_tx {
                        tx.send((address, chain_head_block_number))
                            .await
                            .map_err(|e| tracing::error!("Failed to send pool update: {}", e))
                            .ok();
                    }
                }

                last_synced_block = chain_head_block_number;
            }

            Ok(())
        });

        Ok(updated_pool_handle)
    }

    /// Unwinds the state changes cache for every block from the most recent
    /// state change cache back to the block to unwind -1.
    fn unwind_state_changes(
        pool: &mut EnhancedUniswapPool<Loader, A>,
        state_change_cache: &mut StateChangeCache<Loader, A>,
        block_to_unwind: u64
    ) -> Result<(), PoolManagerError> {
        if let Some(cache) = state_change_cache.get_mut(&pool.address()) {
            loop {
                // check if the most recent state change block is >= the block to unwind
                match cache.get(0) {
                    Some(state_change) if state_change.block_number >= block_to_unwind => {
                        if let Some(option_state_change) = cache.pop_front() {
                            if let Some(pool_state) = option_state_change.state_change {
                                *pool = pool_state;
                            }
                        } else {
                            // We know that there is a state change from cache.get(0) so
                            // when we pop front without returning a value,
                            // there is an issue
                            return Err(PoolManagerError::PopFrontError)
                        }
                    }
                    Some(_) => return Ok(()),
                    None => {
                        // We return an error here because we never want to be unwinding past where
                        // we have state changes. For example, if you
                        // initialize a state space that syncs to block 100,
                        // then immediately after there is a chain reorg to 95,
                        // we can not roll back the state changes for an accurate state
                        // space. In this case, we return an error
                        return Err(PoolManagerError::NoStateChangesInCache)
                    }
                }
            }
        } else {
            Err(PoolManagerError::NoStateChangesInCache)
        }
    }

    fn add_state_change_to_cache(
        state_change_cache: &mut StateChangeCache<Loader, A>,
        state_change: StateChange<Loader, A>,
        address: A
    ) -> Result<(), PoolManagerError> {
        let cache = state_change_cache.entry(address).or_default();
        if cache.is_full() {
            cache.pop_back();
        }
        cache
            .push_front(state_change)
            .map_err(|_| PoolManagerError::CapacityError)
    }

    fn handle_state_changes_from_logs(
        pool: &mut EnhancedUniswapPool<Loader, A>,
        state_change_cache: &mut StateChangeCache<Loader, A>,
        logs: Vec<Log>,
        block_number: BlockNumber
    ) -> Result<(), PoolManagerError> {
        for log in logs {
            pool.sync_from_log(log)?;
        }

        let pool_clone = pool.clone();
        Self::add_state_change_to_cache(
            state_change_cache,
            StateChange::new(Some(pool_clone), block_number),
            pool.address()
        )
    }
}

#[derive(Debug)]
pub struct StateChange<Loader: PoolDataLoader<A>, A> {
    state_change: Option<EnhancedUniswapPool<Loader, A>>,
    block_number: u64
}

impl<Loader: PoolDataLoader<A>, A> StateChange<Loader, A> {
    pub fn new(state_change: Option<EnhancedUniswapPool<Loader, A>>, block_number: u64) -> Self {
        Self { state_change, block_number }
    }
}

#[derive(Error, Debug)]
pub enum PoolManagerError {
    #[error("Invalid block range")]
    InvalidBlockRange,
    #[error("No state changes in cache")]
    NoStateChangesInCache,
    #[error("Error when removing a state change from the front of the deque")]
    PopFrontError,
    #[error("State change cache capacity error")]
    CapacityError,
    #[error(transparent)]
    PoolError(#[from] PoolError),
    #[error("Empty block number of stream")]
    EmptyBlockNumberFromStream,
    #[error(transparent)]
    BlockSendError(#[from] tokio::sync::mpsc::error::SendError<Block>),
    #[error(transparent)]
    JoinError(#[from] tokio::task::JoinError),
    #[error("Synchronization has already been started")]
    SyncAlreadyStarted,
    #[error(transparent)]
    RpcTransportError(#[from] RpcError<TransportErrorKind>)
}
