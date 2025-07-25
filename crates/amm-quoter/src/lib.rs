use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    pin::Pin,
    sync::Arc
};

use alloy::primitives::U160;
use angstrom_types::{
    block_sync::BlockSyncConsumer, primitive::PoolId, uni_structure::BaselinePoolState
};
use futures::{Stream, StreamExt, future::BoxFuture, stream::FuturesUnordered};
use matching_engine::{
    book::{BookOrder, OrderBook},
    build_book
};
use order_pool::order_storage::OrderStorage;
use rayon::ThreadPool;
use serde::{Deserialize, Serialize};
use tokio::{sync::mpsc, time::Interval};
use tokio_stream::wrappers::ReceiverStream;
use uniswap_v4::uniswap::pool_manager::SyncedUniswapPools;

use crate::consensus::ConsensusMode;

mod consensus;
mod rollup;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct Slot0Update {
    /// there will be 120 updates per block or per 100ms
    pub seq_id:           u16,
    /// in case of block lag on node
    pub current_block:    u64,
    pub angstrom_pool_id: PoolId,
    pub uni_pool_id:      PoolId,

    pub sqrt_price_x96: U160,
    pub liquidity:      u128,
    pub tick:           i32
}

pub trait AngstromBookQuoter: Send + Sync + Unpin + 'static {
    /// will configure this stream to receieve updates of the given pool
    fn subscribe_to_updates(
        &self,
        pool_id: HashSet<PoolId>
    ) -> impl Future<Output = Pin<Box<dyn Stream<Item = Slot0Update> + Send + 'static>>> + Send + Sync;
}

pub struct QuoterHandle(pub mpsc::Sender<(HashSet<PoolId>, mpsc::Sender<Slot0Update>)>);

impl AngstromBookQuoter for QuoterHandle {
    async fn subscribe_to_updates(
        &self,
        pool_ids: HashSet<PoolId>
    ) -> Pin<Box<dyn Stream<Item = Slot0Update> + Send + 'static>> {
        let (tx, rx) = mpsc::channel(5);
        let _ = self.0.send((pool_ids, tx)).await;

        ReceiverStream::new(rx).boxed()
    }
}

pub struct RollupMode;

pub struct QuoterManager<BlockSync: BlockSyncConsumer, M = ConsensusMode> {
    cur_block:           u64,
    seq_id:              u16,
    block_sync:          BlockSync,
    orders:              Arc<OrderStorage>,
    amms:                SyncedUniswapPools,
    threadpool:          ThreadPool,
    recv:                mpsc::Receiver<(HashSet<PoolId>, mpsc::Sender<Slot0Update>)>,
    book_snapshots:      HashMap<PoolId, (PoolId, BaselinePoolState)>,
    pending_tasks:       FuturesUnordered<BoxFuture<'static, eyre::Result<Slot0Update>>>,
    pool_to_subscribers: HashMap<PoolId, Vec<mpsc::Sender<Slot0Update>>>,

    execution_interval: Interval,

    mode: M
}

pub fn build_non_proposal_books(
    limit: Vec<BookOrder>,
    pool_snapshots: &HashMap<PoolId, (PoolId, BaselinePoolState)>
) -> HashMap<PoolId, OrderBook> {
    let book_sources = orders_sorted_by_pool_id(limit);

    book_sources
        .into_iter()
        .map(|(id, orders)| {
            let amm = pool_snapshots.get(&id).map(|(_, pool)| pool).cloned();
            (id, build_book(id, amm, orders))
        })
        .collect()
}

pub fn orders_sorted_by_pool_id(limit: Vec<BookOrder>) -> HashMap<PoolId, HashSet<BookOrder>> {
    limit.into_iter().fold(HashMap::new(), |mut acc, order| {
        acc.entry(order.pool_id).or_default().insert(order);
        acc
    })
}
