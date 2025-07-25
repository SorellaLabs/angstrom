use std::{
    collections::{HashMap, HashSet, hash_map::Entry},
    pin::Pin,
    sync::Arc,
    task::Poll,
    time::Duration
};

use angstrom_types::{
    block_sync::BlockSyncConsumer,
    consensus::{ConsensusRoundEvent, ConsensusRoundOrderHashes},
    orders::OrderSet,
    primitive::PoolId,
    sol_bindings::{grouped_orders::AllOrders, rpc_orders::TopOfBlockOrder}
};
use futures::{FutureExt, Stream, StreamExt, TryFutureExt, stream::FuturesUnordered};
use matching_engine::strategy::BinarySearchStrategy;
use order_pool::order_storage::OrderStorage;
use rayon::ThreadPool;
use tokio::{
    sync::{mpsc, oneshot},
    time::interval
};
use uniswap_v4::uniswap::{pool_data_loader::PoolDataLoader, pool_manager::SyncedUniswapPools};

use crate::{QuoterManager, Slot0Update, build_non_proposal_books};

/// Mode for consensus-based order book building.
pub struct ConsensusMode {
    consensus_stream: Pin<Box<dyn Stream<Item = ConsensusRoundOrderHashes> + Send>>,
    /// The unique order hashes of the current PreProposalAggregate consensus
    /// round. Used to build the book for the slot0 stream, so that all
    /// orders are valid, and the subscription can't be manipulated by orders
    /// submitted after this round and between the next block
    active_pre_proposal_aggr_order_hashes: Option<ConsensusRoundOrderHashes>
}

impl<BlockSync: BlockSyncConsumer> QuoterManager<BlockSync, ConsensusMode> {
    /// ensure that we haven't registered on the BlockSync.
    /// We just want to ensure that we don't access during a update period
    pub fn new(
        block_sync: BlockSync,
        orders: Arc<OrderStorage>,
        recv: mpsc::Receiver<(HashSet<PoolId>, mpsc::Sender<Slot0Update>)>,
        amms: SyncedUniswapPools,
        threadpool: ThreadPool,
        update_interval: Duration,
        consensus_stream: Pin<Box<dyn Stream<Item = ConsensusRoundOrderHashes> + Send>>
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

        let mode = ConsensusMode { consensus_stream, active_pre_proposal_aggr_order_hashes: None };

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
            mode
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
        if let Some(hashes) = self.mode.active_pre_proposal_aggr_order_hashes.as_ref() {
            self.orders
                .get_all_orders_with_hashes(&hashes.limit, &hashes.searcher)
        } else {
            self.orders.get_all_orders()
        }
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

    fn update_consensus_state(&mut self, round: ConsensusRoundOrderHashes) {
        if matches!(round.round, ConsensusRoundEvent::PropagatePreProposalAgg) {
            self.mode.active_pre_proposal_aggr_order_hashes = Some(round)
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

impl<BlockSync: BlockSyncConsumer> Future for QuoterManager<BlockSync, ConsensusMode> {
    type Output = ();

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>
    ) -> std::task::Poll<Self::Output> {
        while let Poll::Ready(Some((pools, subscriber))) = self.recv.poll_recv(cx) {
            self.handle_new_subscription(pools, subscriber);
        }

        while let Poll::Ready(Some(consensus_update)) =
            self.mode.consensus_stream.poll_next_unpin(cx)
        {
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

                self.mode.active_pre_proposal_aggr_order_hashes = None;

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
