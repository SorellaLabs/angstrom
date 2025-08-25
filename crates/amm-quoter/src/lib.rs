use std::{
    collections::{HashMap, HashSet, hash_map::Entry},
    fmt::Debug,
    pin::Pin,
    sync::Arc
};

use alloy::primitives::U160;
use angstrom_types::{
    block_sync::BlockSyncConsumer,
    orders::OrderSet,
    primitive::PoolId,
    sol_bindings::{grouped_orders::AllOrders, rpc_orders::TopOfBlockOrder},
    uni_structure::BaselinePoolState
};
use futures::{
    FutureExt, Stream, StreamExt, TryFutureExt, future::BoxFuture, stream::FuturesUnordered
};
use matching_engine::{
    book::{BookOrder, OrderBook},
    build_book,
    strategy::BinarySearchStrategy
};
use order_pool::order_storage::OrderStorage;
use rayon::ThreadPool;
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{mpsc, oneshot},
    time::Interval
};
use tokio_stream::wrappers::ReceiverStream;
use uniswap_v4::uniswap::{pool_data_loader::PoolDataLoader, pool_manager::SyncedUniswapPools};

mod consensus;
mod rollup;

pub use crate::{
    consensus::{ConsensusMode, ConsensusQuoterManager},
    rollup::{RollupMode, RollupQuoterManager}
};

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

impl<BlockSync: BlockSyncConsumer, M> QuoterManager<BlockSync, M> {
    fn handle_new_subscription(&mut self, pools: HashSet<PoolId>, chan: mpsc::Sender<Slot0Update>) {
        let keys = self
            .book_snapshots
            .iter()
            .flat_map(|(ang_key, (uni_key, _))| [ang_key, uni_key])
            .copied()
            .collect::<HashSet<_>>();

        for pool in &pools {
            if !keys.contains(pool) {
                // invalid subscription
                return;
            }
        }

        for pool in pools {
            self.pool_to_subscribers
                .entry(pool)
                .or_default()
                .push(chan.clone());
        }
    }

    fn spawn_book_solvers(&mut self, seq_id: u16, orders: OrderSet<AllOrders, TopOfBlockOrder>) {
        let OrderSet { limit, searcher } = orders;
        let mut books = build_non_proposal_books(limit, &self.book_snapshots);

        let searcher_orders = searcher
            .into_iter()
            .fold(HashMap::new(), |mut acc, searcher| {
                match acc.entry(searcher.pool_id) {
                    Entry::Vacant(v) => {
                        v.insert(searcher);
                    }
                    Entry::Occupied(mut o) => {
                        let current = o.get();
                        // if this order on same pool_id has a higher tob reward or they are the
                        // same and it has a lower order hash. replace
                        if searcher.tob_reward > current.tob_reward
                            || (searcher.tob_reward == current.tob_reward
                                && searcher.order_id.hash < current.order_id.hash)
                        {
                            o.insert(searcher);
                        }
                    }
                };

                acc
            });

        for (book_id, (uni_pool_id, amm)) in &self.book_snapshots {
            // Default as if we don't have a book, we want to still send update.
            let mut book = books.remove(book_id).unwrap_or_default();
            book.id = *book_id;
            book.set_amm_if_missing(|| amm.clone());

            let searcher = searcher_orders.get(&book.id()).cloned();
            let (tx, rx) = oneshot::channel();
            let block = self.cur_block;

            let uni_pool_id = *uni_pool_id;

            self.threadpool.spawn(move || {
                let b = book;
                let (sqrt_price, tick, liquidity) =
                    BinarySearchStrategy::give_end_amm_state(&b, searcher);
                let update = Slot0Update {
                    current_block: block,
                    seq_id,
                    angstrom_pool_id: b.id(),
                    uni_pool_id,
                    liquidity,
                    sqrt_price_x96: sqrt_price,
                    tick
                };

                tx.send(update).unwrap()
            });

            self.pending_tasks.push(rx.map_err(Into::into).boxed())
        }
    }

    fn update_book_state(&mut self) {
        self.book_snapshots = self
            .amms
            .iter()
            .map(|entry| {
                let pool_lock = entry.value().read().unwrap();
                let uni_key = pool_lock.data_loader().private_address();
                let snapshot_data = pool_lock.fetch_pool_snapshot().unwrap().2;
                (*entry.key(), (uni_key, snapshot_data))
            })
            .collect();
    }

    fn send_out_result(&mut self, slot_update: Slot0Update) {
        if let Some(ang_pool_subs) = self
            .pool_to_subscribers
            .get_mut(&slot_update.angstrom_pool_id)
        {
            ang_pool_subs.retain(|subscriber| subscriber.try_send(slot_update.clone()).is_ok());
        };

        if let Some(uni_pool_subs) = self.pool_to_subscribers.get_mut(&slot_update.uni_pool_id) {
            uni_pool_subs.retain(|subscriber| subscriber.try_send(slot_update.clone()).is_ok());
        };
    }
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

pub fn book_snapshots_from_amms(
    amms: &SyncedUniswapPools
) -> HashMap<PoolId, (PoolId, BaselinePoolState)> {
    amms.iter()
        .map(|entry| {
            let pool_lock = entry.value().read().unwrap();
            let uni_key = pool_lock.data_loader().private_address();
            let snapshot_data = pool_lock.fetch_pool_snapshot().unwrap().2;
            (*entry.key(), (uni_key, snapshot_data))
        })
        .collect()
}
