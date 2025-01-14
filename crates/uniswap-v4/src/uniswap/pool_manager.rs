use std::{
    collections::HashMap,
    fmt::Debug,
    future::Future,
    hash::Hash,
    ops::Deref,
    sync::{Arc, RwLock, RwLockReadGuard},
    task::Poll
};

use alloy::{
    primitives::{Address, BlockNumber},
    rpc::types::{eth::Filter, Block},
    transports::{RpcError, TransportErrorKind}
};
use alloy_primitives::Log;
use angstrom_types::{
    block_sync::BlockSyncConsumer,
    contract_payloads::tob::ToBOutcome,
    matching::uniswap::PoolSnapshot,
    primitive::PoolId,
    sol_bindings::{grouped_orders::OrderWithStorageData, rpc_orders::TopOfBlockOrder}
};
use arraydeque::ArrayDeque;
use futures::FutureExt;
use futures_util::{stream::BoxStream, StreamExt};
use thiserror::Error;
use tokio::sync::Notify;

use super::{pool::PoolError, pool_providers::PoolMangerBlocks};
use crate::uniswap::{
    pool::EnhancedUniswapPool,
    pool_data_loader::{DataLoader, PoolDataLoader},
    pool_providers::PoolManagerProvider
};

pub type StateChangeCache<Loader, A> = HashMap<A, ArrayDeque<StateChange<Loader, A>, 150>>;

pub type SyncedUniswapPool<A = PoolId, Loader = DataLoader<A>> =
    Arc<RwLock<EnhancedUniswapPool<Loader, A>>>;

const MODULE_NAME: &str = "UniswapV4";

#[derive(Debug, Clone, Copy)]
pub struct TickRangeToLoad<A = PoolId> {
    pub pool_id:    A,
    pub start_tick: i32,
    pub zfo:        bool,
    pub tick_count: u16
}

type PoolMap<Loader, A> = Arc<HashMap<A, Arc<RwLock<EnhancedUniswapPool<Loader, A>>>>>;

#[derive(Clone)]
pub struct SyncedUniswapPools<A = PoolId, Loader = DataLoader<A>>
where
    Loader: PoolDataLoader<A>
{
    pools: PoolMap<Loader, A>,
    tx:    tokio::sync::mpsc::Sender<(TickRangeToLoad<A>, Arc<Notify>)>
}

impl<A, Loader> Deref for SyncedUniswapPools<A, Loader>
where
    Loader: PoolDataLoader<A>
{
    type Target = PoolMap<Loader, A>;

    fn deref(&self) -> &Self::Target {
        &self.pools
    }
}

/// Amount of ticks to load when we go out of scope;
const OUT_OF_SCOPE_TICKS: u16 = 20;

const ATTEMPTS: u8 = 5;

impl<A, Loader> SyncedUniswapPools<A, Loader>
where
    Loader: PoolDataLoader<A> + Default,
    A: Debug + Hash + PartialEq + Eq + Copy + Default
{
    pub fn new(
        pools: PoolMap<Loader, A>,
        tx: tokio::sync::mpsc::Sender<(TickRangeToLoad<A>, Arc<Notify>)>
    ) -> Self {
        Self { pools, tx }
    }

    /// Will calculate the tob rewards that this order specifies. More Notably,
    /// this function is async and will make sure that we always have the
    /// needed ticks loaded in order to ensure we can always properly
    /// simulate a order.
    pub async fn calculate_rewards(
        &self,
        pool_id: A,
        tob: &OrderWithStorageData<TopOfBlockOrder>
    ) -> eyre::Result<ToBOutcome> {
        tracing::info!("calculate_rewards function");

        let mut cnt = ATTEMPTS;
        loop {
            let market_snapshot = {
                let pool = self.pools.get(&pool_id).unwrap().read().unwrap();
                pool.fetch_pool_snapshot().map(|v| v.2).unwrap()
            };

            let outcome = ToBOutcome::from_tob_and_snapshot(tob, &market_snapshot);

            if outcome.is_err() {
                let zfo = !tob.is_bid;
                let not = Arc::new(Notify::new());
                // scope for awaits
                let start_tick = {
                    let pool = self.pools.get(&pool_id).unwrap().read().unwrap();
                    if zfo {
                        pool.fetch_lowest_tick()
                    } else {
                        pool.fetch_highest_tick()
                    }
                };

                let _ = self
                    .tx
                    .send((
                        // load 50 more ticks on the side of the order and try again
                        TickRangeToLoad {
                            pool_id,
                            start_tick,
                            zfo,
                            tick_count: OUT_OF_SCOPE_TICKS
                        },
                        not.clone()
                    ))
                    .await;

                not.notified().await;

                // don't loop forever
                cnt -= 1;
                if cnt == 0 {
                    return outcome
                }

                continue
            }
            return outcome
        }
    }
}

pub struct UniswapPoolManager<P, BlockSync, Loader: PoolDataLoader<A>, A = Address>
where
    A: Debug + Copy
{
    pools:               SyncedUniswapPools<A, Loader>,
    latest_synced_block: u64,
    state_change_cache:  Arc<RwLock<StateChangeCache<Loader, A>>>,
    provider:            Arc<P>,
    block_sync:          BlockSync,
    block_stream:        BoxStream<'static, Option<PoolMangerBlocks>>,
    rx:                  tokio::sync::mpsc::Receiver<(TickRangeToLoad<A>, Arc<Notify>)>
}

impl<P, BlockSync, Loader, A> UniswapPoolManager<P, BlockSync, Loader, A>
where
    A: Eq + Hash + Debug + Default + Copy + Sync + Send + 'static,
    Loader: PoolDataLoader<A> + Default + Clone + Send + Sync + 'static,
    BlockSync: BlockSyncConsumer,
    P: PoolManagerProvider + Send + Sync + 'static
{
    pub fn new(
        pools: Vec<EnhancedUniswapPool<Loader, A>>,
        latest_synced_block: BlockNumber,
        provider: Arc<P>,
        block_sync: BlockSync
    ) -> Self {
        block_sync.register(MODULE_NAME);

        let rwlock_pools = pools
            .into_iter()
            .map(|pool| (pool.address(), Arc::new(RwLock::new(pool))))
            .collect();

        let block_stream = <P as Clone>::clone(&provider);
        let block_stream = block_stream.subscribe_blocks();
        let (tx, rx) = tokio::sync::mpsc::channel(100);

        Self {
            pools: SyncedUniswapPools::new(Arc::new(rwlock_pools), tx),
            latest_synced_block,
            state_change_cache: Arc::new(RwLock::new(HashMap::new())),
            block_stream,
            provider,
            block_sync,
            rx
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

    pub fn pools(&self) -> SyncedUniswapPools<A, Loader> {
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

    fn handle_new_block_info(&mut self, block_info: PoolMangerBlocks) {
        // If there is a reorg, unwind state changes from last_synced block to the
        // chain head block number
        let (chain_head_block_number, block_range, is_reorg) = match block_info {
            PoolMangerBlocks::NewBlock(block) => (block, None, false),
            PoolMangerBlocks::Reorg(tip, range) => {
                self.latest_synced_block = tip - range.end();
                tracing::trace!(
                    tip,
                    self.latest_synced_block,
                    "reorg detected, unwinding state changes"
                );
                (tip, Some(range), true)
            }
        };

        let logs = self
            .provider
            .get_logs(
                &self
                    .filter()
                    .from_block(self.latest_synced_block + 1)
                    .to_block(chain_head_block_number)
            )
            .expect("should never fail");

        if is_reorg {
            // scope for locks
            let mut state_change_cache = self.state_change_cache.write().unwrap();
            for pool in self.pools.values() {
                let mut pool_guard = pool.write().unwrap();
                Self::unwind_state_changes(
                    &mut pool_guard,
                    &mut state_change_cache,
                    chain_head_block_number
                )
                .expect("should never fail");
            }
        }

        let logs_by_address = Loader::group_logs(logs);

        for (addr, logs) in logs_by_address {
            if logs.is_empty() {
                continue
            }

            let Some(pool) = self.pools.get(&addr) else {
                continue;
            };

            let mut pool_guard = pool.write().unwrap();
            let mut state_change_cache = self.state_change_cache.write().unwrap();
            Self::handle_state_changes_from_logs(
                &mut pool_guard,
                &mut state_change_cache,
                logs,
                chain_head_block_number
            )
            .expect("never fail");
        }

        self.latest_synced_block = chain_head_block_number;

        if is_reorg {
            self.block_sync
                .sign_off_reorg(MODULE_NAME, block_range.unwrap(), None);
        } else {
            self.block_sync
                .sign_off_on_block(MODULE_NAME, self.latest_synced_block, None);
        }
    }

    #[allow(clippy::await_holding_lock)]
    async fn load_more_ticks(
        notifier: Arc<Notify>,
        pools: SyncedUniswapPools<A, Loader>,
        provider: Arc<P>,
        tick_req: TickRangeToLoad<A>
    ) {
        let node_provider = provider.provider();
        let mut pool = pools.get(&tick_req.pool_id).unwrap().write().unwrap();

        // given we force this to resolve, should'nt be problematic
        let ticks = pool
            .load_more_ticks(tick_req, None, node_provider)
            .await
            .unwrap();

        pool.apply_ticks(ticks);

        // notify we have updated the liquidity
        notifier.notify_one();
    }
}

impl<P, BlockSync, Loader, A> Future for UniswapPoolManager<P, BlockSync, Loader, A>
where
    A: Eq + Hash + Debug + Default + Copy + Sync + Send + Unpin + 'static,
    Loader: PoolDataLoader<A> + Default + Clone + Send + Sync + Unpin + 'static,
    BlockSync: BlockSyncConsumer,
    P: PoolManagerProvider + Send + Sync + 'static
{
    type Output = ();

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>
    ) -> std::task::Poll<Self::Output> {
        while let Poll::Ready(Some(Some(block_info))) = self.block_stream.poll_next_unpin(cx) {
            self.handle_new_block_info(block_info);
        }
        while let Poll::Ready(Some((ticks, not))) = self.rx.poll_recv(cx) {
            // hacky for now but only way to avoid lock problems
            let pools = self.pools.clone();
            let prov = self.provider.clone();
            let mut f = Box::pin(Self::load_more_ticks(not, pools, prov, ticks));

            while f.poll_unpin(cx).is_pending() {}
        }

        Poll::Pending
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
