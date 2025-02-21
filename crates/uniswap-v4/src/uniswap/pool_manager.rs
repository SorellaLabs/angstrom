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
    /// the poolId with the fee to the dynamic fee poolId
    conversion_map:      HashMap<A, A>,
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
        conversion_map: HashMap<A, A>,
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
            conversion_map,
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
                // gotta
                Some((
                    self.convert_to_pub_id(key),
                    pool.read().unwrap().fetch_pool_snapshot().ok()?.2
                ))
            })
            .collect()
    }

    pub fn pool_addresses(&self) -> impl Iterator<Item = A> + '_ {
        self.pools.keys().map(|k| self.convert_to_pub_id(k))
    }

    pub fn pools(&self) -> SyncedUniswapPools<A, Loader> {
        let mut c = self.pools.clone();
        c.pools = Arc::new(
            c.pools
                .iter()
                .map(|(k, v)| (self.convert_to_pub_id(k), v.clone()))
                .collect()
        );

        c
    }

    fn convert_to_pub_id(&self, key: &A) -> A {
        self.conversion_map
            .iter()
            .find_map(|(r, m)| {
                if m == key {
                    return Some(r)
                }
                None
            })
            .copied()
            .unwrap()
    }

    pub fn pool(&self, address: &A) -> Option<RwLockReadGuard<'_, EnhancedUniswapPool<Loader, A>>> {
        let addr = self.conversion_map.get(address)?;
        let pool = self.pools.get(addr)?;
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
                        tracing::warn!(addr=?pool.address(),"cache.get(0) == None");
                        return Err(PoolManagerError::NoStateChangesInCache)
                    }
                }
            }
        } else {
            tracing::warn!("get_mut failed");
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
                // Handle potential overflow by ensuring we don't go below 0
                self.latest_synced_block = tip.saturating_sub(*range.end());
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

        Self::pool_update_workaround(
            chain_head_block_number,
            self.pools.clone(),
            self.provider.clone()
        );

        self.latest_synced_block = chain_head_block_number;

        if is_reorg {
            self.block_sync
                .sign_off_reorg(MODULE_NAME, block_range.unwrap(), None);
        } else {
            self.block_sync
                .sign_off_on_block(MODULE_NAME, self.latest_synced_block, None);
        }
    }

    fn pool_update_workaround(
        block_number: u64,
        pools: SyncedUniswapPools<A, Loader>,
        provider: Arc<P>
    ) {
        tracing::info!("starting poll");
        for pool in pools.pools.values() {
            let mut l = pool.write().unwrap();
            async_to_sync(l.update_to_block(Some(block_number), provider.provider())).unwrap();
        }
        tracing::info!("finished");
    }

    fn load_more_ticks(
        notifier: Arc<Notify>,
        pools: SyncedUniswapPools<A, Loader>,
        provider: Arc<P>,
        tick_req: TickRangeToLoad<A>
    ) {
        let node_provider = provider.provider();
        let mut pool = pools.get(&tick_req.pool_id).unwrap().write().unwrap();

        // given we force this to resolve, should'nt be problematic
        let ticks = async_to_sync(pool.load_more_ticks(tick_req, None, node_provider)).unwrap();

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
        while let Poll::Ready(Some((mut ticks, not))) = self.rx.poll_recv(cx) {
            // hacky for now but only way to avoid lock problems
            let pools = self.pools.clone();
            let prov = self.provider.clone();

            let addr = self.conversion_map.get(&ticks.pool_id).unwrap();
            ticks.pool_id = addr.clone();
            Self::load_more_ticks(not, pools, prov, ticks);
        }

        Poll::Pending
    }
}

pub fn async_to_sync<F: Future>(f: F) -> F::Output {
    let handle = tokio::runtime::Handle::try_current().expect("No tokio runtime found");
    tokio::task::block_in_place(|| handle.block_on(f))
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

#[cfg(test)]
mod annoying_tests {
    use std::{sync::Arc, task::Waker};

    use alloy::{
        primitives::Address,
        providers::{fillers::*, network::Ethereum, Provider, ProviderBuilder, RootProvider, *}
    };
    use alloy_primitives::LogData;
    use angstrom_types::block_sync::GlobalBlockState;
    use tokio::sync::mpsc;

    use super::*;

    type ProviderDef = FillProvider<
        JoinFill<
            Identity,
            JoinFill<GasFiller, JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>>
        >,
        RootProvider,
        Ethereum
    >;
    // Mock implementations for testing
    #[derive(Clone)]
    struct MockProvider {
        logs: Arc<RwLock<Vec<Log>>>,
        p:    ProviderDef
    }

    impl MockProvider {
        async fn new() -> Self {
            Self {
                logs: Arc::new(RwLock::new(Vec::new())),
                p:    ProviderBuilder::new()
                    .on_builtin("https://eth.llamarpc.com")
                    .await
                    .unwrap()
            }
        }

        fn add_logs(&self, logs: Vec<Log>) {
            let mut guard = self.logs.write().unwrap();
            guard.extend(logs);
        }
    }
    impl PoolManagerProvider for MockProvider {
        fn subscribe_blocks(self) -> BoxStream<'static, Option<PoolMangerBlocks>> {
            let (_, rx) = mpsc::channel(1);
            Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx))
        }

        fn get_logs(&self, _filter: &Filter) -> Result<Vec<Log>, PoolManagerError> {
            Ok(self.logs.read().unwrap().clone())
        }

        fn provider(&self) -> Arc<impl Provider> {
            Arc::new(self.clone())
        }
    }

    impl Provider for MockProvider {
        fn root(&self) -> &RootProvider<Ethereum> {
            self.p.root()
        }
    }

    #[derive(Debug, Clone)]
    struct MockBlockSync;

    impl BlockSyncConsumer for MockBlockSync {
        fn sign_off_reorg(
            &self,
            _module: &'static str,
            _range: std::ops::RangeInclusive<u64>,
            _data: Option<Waker>
        ) {
        }

        fn sign_off_on_block(&self, _module: &'static str, _block: u64, _data: Option<Waker>) {}

        fn current_block_number(&self) -> u64 {
            0
        }

        fn has_proposal(&self) -> bool {
            false
        }

        fn fetch_current_proposal(&self) -> Option<GlobalBlockState> {
            None
        }

        fn register(&self, _module: &'static str) {}
    }

    #[tokio::test]
    async fn test_handle_new_block() {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_test_writer()
            .try_init();

        tracing::info!("Starting test_handle_new_block");
        let provider = Arc::new(MockProvider::new().await);
        let block_sync = MockBlockSync;

        // Create a mock pool
        let pool = EnhancedUniswapPool::<DataLoader<PoolId>, PoolId>::default();
        let pool_id = PoolId::default();

        let mut map = HashMap::new();
        map.insert(pool_id, pool_id);

        let mut manager = UniswapPoolManager::new(
            vec![pool],
            map,
            100, // Start at block 100
            provider.clone(),
            block_sync
        );

        // Initialize the state change cache with an empty state change
        {
            let mut cache = manager.state_change_cache.write().unwrap();
            StateChange::<DataLoader<PoolId>, PoolId>::new(None, 100);
            cache.insert(pool_id, ArrayDeque::new());
        }

        let log = Log { address: Address::default(), data: LogData::default() };
        provider.add_logs(vec![log]);

        // Process new block
        manager.handle_new_block_info(PoolMangerBlocks::NewBlock(101));

        // Verify state was updated
        assert_eq!(manager.latest_synced_block, 101);

        // Verify state changes were cached
        let cache = manager.state_change_cache.read().unwrap();
        assert!(cache.contains_key(&pool_id));
    }

    /// NOTE: when reorgs occur, lets say we reorg back 2 blocks from 100 to 98,
    /// the system will roll back to block 97.
    #[tokio::test]
    async fn test_handle_reorg() {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_test_writer()
            .try_init();

        tracing::info!("Starting test_handle_reorg");
        let provider = Arc::new(MockProvider::new().await);
        let block_sync = MockBlockSync;

        // Create a mock pool with a specific pool_id
        let pool = EnhancedUniswapPool::<DataLoader<PoolId>, PoolId>::default();
        let pool_id = PoolId::default();

        let mut map = HashMap::new();
        map.insert(pool_id, pool_id);

        let mut manager = UniswapPoolManager::new(
            vec![pool],
            map,
            95, // Start at block 95
            provider.clone(),
            block_sync
        );

        // Initialize the state change cache with multiple state changes
        {
            tracing::info!("Initializing state change cache");
            let mut cache = manager.state_change_cache.write().unwrap();
            let mut deque: ArrayDeque<StateChange<DataLoader<PoolId>, PoolId>, 150> =
                ArrayDeque::new();

            // Create distinct mock pool states for each block
            for block in 95..=100 {
                // Reverse order to match how real state changes would be added
                tracing::debug!("Creating mock pool state for block {}", block);
                let mut mock_pool = EnhancedUniswapPool::<DataLoader<PoolId>, PoolId>::default();
                mock_pool.set_sqrt_price_x96(block as u128 * 1_000_000);

                deque
                    .push_front(StateChange::new(Some(mock_pool), block))
                    .expect("Failed to add state");
            }
            tracing::info!(?pool_id, "Inserting state changes into cache");
            cache.insert(pool_id, deque);
            tracing::info!(len=?cache.len());
        }

        manager.latest_synced_block = 100;

        tracing::info!("Triggering reorg from block 100 back to 95");
        manager.handle_new_block_info(PoolMangerBlocks::Reorg(96, 96..=100));

        // Verify state was rolled back
        tracing::info!("Verifying state rollback");
        assert_eq!(manager.latest_synced_block, 96);

        // Verify state changes reflect reorg
        tracing::info!("Verifying state changes after reorg");
        let cache = manager.state_change_cache.read().unwrap();
        if let Some(changes) = cache.get(&pool_id) {
            // Verify we only have states up to block 96
            tracing::debug!("Verifying all state changes are <= block 96");
            assert!(changes.iter().all(|change| change.block_number <= 96));

            // Verify the current pool state matches block 95 given the reorg starts at 96
            tracing::debug!("Verifying current pool state matches block 96");
            if let Some(front) = changes.front() {
                if let Some(state) = &front.state_change {
                    assert_eq!(state.get_sqrt_price_x96(), 95 * 1_000_000);
                    assert_eq!(front.block_number, 95);
                }
            }
        }
    }
}
