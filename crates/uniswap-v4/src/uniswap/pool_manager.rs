use std::{
    collections::HashMap,
    fmt::Debug,
    future::Future,
    ops::Deref,
    pin::Pin,
    sync::{Arc, RwLock},
    task::Poll
};

use alloy::{
    primitives::BlockNumber,
    providers::Provider,
    rpc::types::Block,
    transports::{RpcError, TransportErrorKind}
};
use angstrom_eth::manager::EthEvent;
use angstrom_types::{
    block_sync::BlockSyncConsumer,
    contract_payloads::angstrom::TopOfBlockOrder as PayloadTopOfBlockOrder,
    matching::uniswap::PoolSnapshot,
    primitive::PoolId,
    sol_bindings::{grouped_orders::OrderWithStorageData, rpc_orders::TopOfBlockOrder}
};
use arraydeque::ArrayDeque;
use dashmap::DashMap;
use futures::Stream;
use futures_util::{StreamExt, stream::BoxStream};
use thiserror::Error;
use tokio::sync::Notify;

use super::{pool::PoolError, pool_factory::V4PoolFactory, pool_providers::PoolMangerBlocks};
use crate::uniswap::{
    pool::EnhancedUniswapPool,
    pool_data_loader::{DataLoader, PoolDataLoader},
    pool_providers::PoolManagerProvider
};

pub type StateChangeCache = HashMap<PoolId, ArrayDeque<StateChange, 150>>;

pub type SyncedUniswapPool<Loader = DataLoader> = Arc<RwLock<EnhancedUniswapPool<Loader>>>;

const MODULE_NAME: &str = "UniswapV4";

#[derive(Debug, Clone, Copy)]
pub struct TickRangeToLoad {
    pub pool_id:    PoolId,
    pub start_tick: i32,
    pub zfo:        bool,
    pub tick_count: u16
}

impl TickRangeToLoad {
    pub fn end_tick(&self, spacing: i32) -> i32 {
        if self.zfo {
            self.start_tick - (self.tick_count as i32 * spacing)
        } else {
            self.start_tick + (self.tick_count as i32 * spacing)
        }
    }
}

type PoolMap = Arc<DashMap<PoolId, Arc<RwLock<EnhancedUniswapPool<DataLoader>>>>>;

#[derive(Clone)]
pub struct SyncedUniswapPools
where
    DataLoader: PoolDataLoader
{
    pools: PoolMap,
    tx:    tokio::sync::mpsc::Sender<(TickRangeToLoad, Arc<Notify>)>
}

impl Deref for SyncedUniswapPools
where
    DataLoader: PoolDataLoader
{
    type Target = PoolMap;

    fn deref(&self) -> &Self::Target {
        &self.pools
    }
}

/// Amount of ticks to load when we go out of scope;
const OUT_OF_SCOPE_TICKS: u16 = 20;

const ATTEMPTS: u8 = 5;

impl SyncedUniswapPools
where
    DataLoader: PoolDataLoader
{
    pub fn new(
        pools: PoolMap,
        tx: tokio::sync::mpsc::Sender<(TickRangeToLoad, Arc<Notify>)>
    ) -> Self {
        Self { pools, tx }
    }

    /// Will calculate the tob rewards that this order specifies. More Notably,
    /// this function is async and will make sure that we always have the
    /// needed ticks loaded in order to ensure we can always properly
    /// simulate a order.
    pub async fn calculate_rewards(
        &self,
        pool_id: PoolId,
        tob: &OrderWithStorageData<TopOfBlockOrder>
    ) -> eyre::Result<u128> {
        tracing::info!("calculate_rewards function");

        let mut cnt = ATTEMPTS;
        loop {
            let market_snapshot = {
                let p_lock = self.pools.get(&pool_id).unwrap();
                let pool = p_lock.read().unwrap();
                pool.fetch_pool_snapshot().map(|v| v.2).unwrap()
            };

            let outcome =
                PayloadTopOfBlockOrder::calc_vec_and_reward(tob, &market_snapshot).map(|(_, r)| r);
            tracing::info!(?outcome);

            if outcome.is_err() {
                let zfo = !tob.is_bid;
                let not = Arc::new(Notify::new());
                // scope for awaits
                let start_tick = {
                    let p_lock = self.pools.get(&pool_id).unwrap();
                    let pool = p_lock.read().unwrap();
                    if zfo { pool.fetch_lowest_tick() } else { pool.fetch_highest_tick() }
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
                    return outcome;
                }

                continue;
            }
            return outcome;
        }
    }
}

pub struct UniswapPoolManager<P, PP, BlockSync>
where
    DataLoader: PoolDataLoader,
    PP: Provider + 'static
{
    /// the poolId with the fee to the dynamic fee poolId
    factory:             V4PoolFactory<PP>,
    pools:               SyncedUniswapPools,
    latest_synced_block: u64,
    _state_change_cache: Arc<RwLock<StateChangeCache>>,
    provider:            Arc<P>,
    block_sync:          BlockSync,
    block_stream:        BoxStream<'static, Option<PoolMangerBlocks>>,
    update_stream:       Pin<Box<dyn Stream<Item = EthEvent> + Send + Sync>>,
    rx:                  tokio::sync::mpsc::Receiver<(TickRangeToLoad, Arc<Notify>)>
}

impl<P, PP, BlockSync> UniswapPoolManager<P, PP, BlockSync>
where
    PP: Provider + 'static,
    DataLoader: PoolDataLoader + Default + Clone + Send + Sync + Unpin + 'static,
    BlockSync: BlockSyncConsumer,
    P: PoolManagerProvider + Send + Sync + 'static
{
    pub async fn new(
        factory: V4PoolFactory<PP>,
        latest_synced_block: BlockNumber,
        provider: Arc<P>,
        block_sync: BlockSync,
        update_stream: Pin<Box<dyn Stream<Item = EthEvent> + Send + Sync>>
    ) -> Self {
        block_sync.register(MODULE_NAME);

        let rwlock_pools = factory
            .init(latest_synced_block)
            .await
            .into_iter()
            .map(|pool| (pool.address(), Arc::new(RwLock::new(pool))))
            .collect();

        let block_stream = <P as Clone>::clone(&provider);
        let block_stream = block_stream.subscribe_blocks();
        let (tx, rx) = tokio::sync::mpsc::channel(100);

        Self {
            factory,
            pools: SyncedUniswapPools::new(Arc::new(rwlock_pools), tx),
            latest_synced_block,
            _state_change_cache: Arc::new(RwLock::new(HashMap::new())),
            block_stream,
            provider,
            block_sync,
            update_stream,
            rx
        }
    }

    pub fn fetch_pool_snapshots(&self) -> HashMap<PoolId, PoolSnapshot> {
        self.pools
            .iter()
            .filter_map(|refr| {
                let key = refr.key();
                let pool = refr.value();
                // gotta
                Some((
                    self.convert_to_pub_id(key),
                    pool.read().unwrap().fetch_pool_snapshot().ok()?.2
                ))
            })
            .collect()
    }

    pub fn pool_addresses(&self) -> impl Iterator<Item = PoolId> + '_ {
        self.pools.iter().map(|k| self.convert_to_pub_id(k.key()))
    }

    pub fn pools(&self) -> SyncedUniswapPools {
        let mut c = self.pools.clone();
        c.pools = Arc::new(
            c.pools
                .iter()
                .map(|refr| {
                    let k = refr.key();
                    let v = refr.value();

                    (self.convert_to_pub_id(k), v.clone())
                })
                .collect()
        );

        c
    }

    fn convert_to_pub_id(&self, key: &PoolId) -> PoolId {
        self.factory
            .conversion_map()
            .iter()
            .find_map(|(r, m)| {
                if m == key {
                    return Some(r);
                }
                None
            })
            .copied()
            .unwrap()
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

    fn pool_update_workaround(block_number: u64, pools: SyncedUniswapPools, provider: Arc<P>) {
        tracing::info!("starting poll");
        for pool in pools.pools.iter() {
            let pool = pool.value();
            let mut l = pool.write().unwrap();
            async_to_sync(l.update_to_block(Some(block_number), provider.provider())).unwrap();
        }
        tracing::info!("finished");
    }

    fn load_more_ticks(
        notifier: Arc<Notify>,
        pools: SyncedUniswapPools,
        provider: Arc<P>,
        tick_req: TickRangeToLoad
    ) {
        let node_provider = provider.provider();
        let binding = pools.get(&tick_req.pool_id).unwrap();
        let mut pool = binding.write().unwrap();

        // given we force this to resolve, should'nt be problematic
        async_to_sync(pool.load_more_ticks(tick_req, None, node_provider)).unwrap();

        // notify we have updated the liquidity
        notifier.notify_one();
    }
}

impl<P, PP, BlockSync> Future for UniswapPoolManager<P, PP, BlockSync>
where
    DataLoader: PoolDataLoader + Default + Clone + Send + Sync + Unpin + 'static,
    BlockSync: BlockSyncConsumer,
    P: PoolManagerProvider + Send + Sync + 'static,
    PP: Provider + 'static
{
    type Output = ();

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>
    ) -> std::task::Poll<Self::Output> {
        while let Poll::Ready(Some(Some(block_info))) = self.block_stream.poll_next_unpin(cx) {
            self.handle_new_block_info(block_info);
        }
        while let Poll::Ready(Some(event)) = self.update_stream.poll_next_unpin(cx) {
            match event {
                EthEvent::NewPool { pool } => {
                    let block = self.latest_synced_block;
                    let pool =
                        async_to_sync(self.factory.create_new_angstrom_pool(pool.clone(), block));
                    let key = pool.address();
                    self.pools.insert(key, Arc::new(RwLock::new(pool)));
                }
                EthEvent::RemovedPool { pool } => {
                    let id = self.factory.remove_pool(pool);
                    self.pools.remove(&id);
                }
                _ => {}
            }
        }

        while let Poll::Ready(Some((mut ticks, not))) = self.rx.poll_recv(cx) {
            // hacky for now but only way to avoid lock problems
            let pools = self.pools.clone();
            let prov = self.provider.clone();

            let addr = self.factory.conversion_map().get(&ticks.pool_id).unwrap();
            ticks.pool_id = *addr;
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
pub struct StateChange
where
    DataLoader: PoolDataLoader
{
    _state_change: Option<EnhancedUniswapPool<DataLoader>>,
    _block_number: u64
}

impl StateChange
where
    DataLoader: PoolDataLoader
{
    pub fn new(_state_change: Option<EnhancedUniswapPool<DataLoader>>, _block_number: u64) -> Self {
        Self { _state_change, _block_number }
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
