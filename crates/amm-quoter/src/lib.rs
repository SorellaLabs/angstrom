use std::{
    collections::{HashMap, HashSet, hash_map::Entry},
    fmt::Debug,
    pin::Pin,
    sync::Arc,
    task::Poll,
    time::Duration
};

use alloy::primitives::U160;
#[cfg(not(feature = "rollup"))]
use angstrom_types::consensus::{ConsensusRoundEvent, ConsensusRoundOrderHashes};
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
    time::{Interval, interval}
};
use tokio_stream::wrappers::ReceiverStream;
use uniswap_v4::uniswap::{pool_data_loader::PoolDataLoader, pool_manager::SyncedUniswapPools};

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

pub struct QuoterManager<BlockSync: BlockSyncConsumer> {
    cur_block: u64,
    seq_id: u16,
    block_sync: BlockSync,
    orders: Arc<OrderStorage>,
    amms: SyncedUniswapPools,
    threadpool: ThreadPool,
    recv: mpsc::Receiver<(HashSet<PoolId>, mpsc::Sender<Slot0Update>)>,
    book_snapshots: HashMap<PoolId, (PoolId, BaselinePoolState)>,
    pending_tasks: FuturesUnordered<BoxFuture<'static, eyre::Result<Slot0Update>>>,
    pool_to_subscribers: HashMap<PoolId, Vec<mpsc::Sender<Slot0Update>>>,
    #[cfg(not(feature = "rollup"))]
    consensus_stream: Pin<Box<dyn Stream<Item = ConsensusRoundOrderHashes> + Send>>,
    /// The unique order hashes of the current PreProposalAggregate consensus
    /// round. Used to build the book for the slot0 stream, so that all
    /// orders are valid, and the subscription can't be manipulated by orders
    /// submitted after this round and between the next block
    #[cfg(not(feature = "rollup"))]
    active_pre_proposal_aggr_order_hashes: Option<ConsensusRoundOrderHashes>,

    execution_interval: Interval
}

impl<BlockSync: BlockSyncConsumer> QuoterManager<BlockSync> {
    /// ensure that we haven't registered on the BlockSync.
    /// We just want to ensure that we don't access during a update period
    pub fn new(
        block_sync: BlockSync,
        orders: Arc<OrderStorage>,
        recv: mpsc::Receiver<(HashSet<PoolId>, mpsc::Sender<Slot0Update>)>,
        amms: SyncedUniswapPools,
        threadpool: ThreadPool,
        update_interval: Duration,
        #[cfg(not(feature = "rollup"))] consensus_stream: Pin<
            Box<dyn Stream<Item = ConsensusRoundOrderHashes> + Send>
        >
    ) -> Self {
        let cur_block = block_sync.current_block_number();
        let book_snapshots = amms
            .iter()
            .map(|entry| {
                let pool_lock = entry.value().read().unwrap();

                let pk = pool_lock.public_address();
                let uni_key = pool_lock.data_loader().private_address();
                let snapshot_data = pool_lock.fetch_pool_snapshot().unwrap().2;
                (pk, (uni_key, snapshot_data))
            })
            .collect();

        assert!(
            update_interval > Duration::from_millis(10),
            "cannot update quicker than every 10ms"
        );

        Self {
            seq_id: 0,
            block_sync,
            orders,
            amms,
            recv,
            cur_block,
            book_snapshots,
            threadpool,
            pending_tasks: FuturesUnordered::new(),
            pool_to_subscribers: HashMap::default(),
            execution_interval: interval(update_interval),
            #[cfg(not(feature = "rollup"))]
            consensus_stream,
            #[cfg(not(feature = "rollup"))]
            active_pre_proposal_aggr_order_hashes: None
        }
    }

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

    fn all_orders_with_consensus(&self) -> OrderSet<AllOrders, TopOfBlockOrder> {
        #[cfg(not(feature = "rollup"))]
        if let Some(hashes) = self.active_pre_proposal_aggr_order_hashes.as_ref() {
            self.orders
                .get_all_orders_with_hashes(&hashes.limit, &hashes.searcher)
        } else {
            self.orders.get_all_orders()
        }

        #[cfg(feature = "rollup")]
        self.orders.get_all_orders()
    }

    fn spawn_book_solvers(&mut self, seq_id: u16) {
        let OrderSet { limit, searcher } = self.all_orders_with_consensus();
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

    #[cfg(not(feature = "rollup"))]
    fn update_consensus_state(&mut self, round: ConsensusRoundOrderHashes) {
        if matches!(round.round, ConsensusRoundEvent::PropagatePreProposalAgg) {
            self.active_pre_proposal_aggr_order_hashes = Some(round)
        }
    }

    fn send_out_result(&mut self, slot_update: Slot0Update) {
        if let Some(ang_pool_subs) = self
            .pool_to_subscribers
            .get_mut(&slot_update.angstrom_pool_id)
        {
            ang_pool_subs.retain(|subscriber| subscriber.try_send(slot_update.clone()).is_ok());
        };

        if let Some(uni_pool_subs) = self
            .pool_to_subscribers
            .get_mut(&slot_update.angstrom_pool_id)
        {
            uni_pool_subs.retain(|subscriber| subscriber.try_send(slot_update.clone()).is_ok());
        };
    }
}

impl<BlockSync: BlockSyncConsumer> Future for QuoterManager<BlockSync> {
    type Output = ();

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>
    ) -> std::task::Poll<Self::Output> {
        while let Poll::Ready(Some((pools, subscriber))) = self.recv.poll_recv(cx) {
            self.handle_new_subscription(pools, subscriber);
        }

        #[cfg(not(feature = "rollup"))]
        while let Poll::Ready(Some(consensus_update)) = self.consensus_stream.poll_next_unpin(cx) {
            self.update_consensus_state(consensus_update);
        }

        while let Poll::Ready(Some(Ok(slot_update))) = self.pending_tasks.poll_next_unpin(cx) {
            self.send_out_result(slot_update);
        }

        while self.execution_interval.poll_tick(cx).is_ready() {
            // cycle through if we can't do any processing
            if !self.block_sync.can_operate() {
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }

            // update block number, amm snapshot and reset seq id
            if self.cur_block != self.block_sync.current_block_number() {
                self.update_book_state();
                self.cur_block = self.block_sync.current_block_number();

                #[cfg(not(feature = "rollup"))]
                {
                    self.active_pre_proposal_aggr_order_hashes = None;
                }

                self.seq_id = 0;
            }

            // inc seq_id
            let seq_id = self.seq_id;
            // given that we have a max update speed of 10ms, the max
            // this should reach is 1200 before a new block update
            // occurs. Becuase of this, there is no need to check for overflow
            // as 65535 is more than enough
            self.seq_id += 1;

            self.spawn_book_solvers(seq_id);
        }

        Poll::Pending
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
