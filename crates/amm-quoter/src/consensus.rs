//! This module implements consensus-based quoter mode.
use std::{
    collections::{HashMap, HashSet},
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
use futures::{Stream, StreamExt, stream::FuturesUnordered};
use order_pool::order_storage::OrderStorage;
use rayon::ThreadPool;
use tokio::{sync::mpsc, time::interval};
use uniswap_v4::uniswap::pool_manager::SyncedUniswapPools;

use crate::{QuoterManager, Slot0Update, book_snapshots_from_amms};

/// A type alias for the consensus quoter manager.
pub type ConsensusQuoterManager<BlockSync> = QuoterManager<BlockSync, ConsensusMode>;

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
        let book_snapshots = book_snapshots_from_amms(&amms);

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

    pub(crate) fn all_orders_with_consensus(&self) -> OrderSet<AllOrders, TopOfBlockOrder> {
        if let Some(hashes) = self.mode.active_pre_proposal_aggr_order_hashes.as_ref() {
            self.orders
                .get_all_orders_with_hashes(&hashes.limit, &hashes.searcher)
        } else {
            self.orders.get_all_orders()
        }
    }

    fn update_consensus_state(&mut self, round: ConsensusRoundOrderHashes) {
        if matches!(round.round, ConsensusRoundEvent::PropagatePreProposalAgg) {
            self.mode.active_pre_proposal_aggr_order_hashes = Some(round)
        }
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

        // Drain all ready tasks, forwarding successes and logging errors.
        while let Poll::Ready(Some(result)) = self.pending_tasks.poll_next_unpin(cx) {
            match result {
                Ok(slot_update) => self.send_out_result(slot_update),
                Err(e) => tracing::error!("task failed: {}", e)
            }
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

            let orders = self.all_orders_with_consensus();
            self.spawn_book_solvers(seq_id, orders);
        }

        Poll::Pending
    }
}
