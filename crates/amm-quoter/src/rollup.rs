//! This module implements rollup-based quoter mode.
//!
//! ## Differences with Consensus Mode
//! - No consensus updates
//! - All orders are considered, not just the ones with consensus
use std::{
    collections::{HashMap, HashSet},
    pin::Pin,
    sync::Arc,
    task::Poll,
    time::Duration
};

use angstrom_types::{block_sync::BlockSyncConsumer, primitive::PoolId};
use futures::{StreamExt, stream::FuturesUnordered};
use order_pool::order_storage::OrderStorage;
use rayon::ThreadPool;
use tokio::{sync::mpsc, time::interval};
use uniswap_v4::uniswap::pool_manager::SyncedUniswapPools;

use crate::{QuoterManager, Slot0Update, book_snapshots_from_amms};

/// Mode for rollup-based order book building.
pub struct RollupMode;

impl<BlockSync: BlockSyncConsumer> QuoterManager<BlockSync, RollupMode> {
    pub fn new(
        block_sync: BlockSync,
        orders: Arc<OrderStorage>,
        recv: mpsc::Receiver<(HashSet<PoolId>, mpsc::Sender<Slot0Update>)>,
        amms: SyncedUniswapPools,
        threadpool: ThreadPool,
        update_interval: Duration
    ) -> Self {
        let cur_block = block_sync.current_block_number();
        let book_snapshots = book_snapshots_from_amms(&amms);

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
            mode: RollupMode
        }
    }
}

impl<BlockSync: BlockSyncConsumer> Future for QuoterManager<BlockSync, RollupMode> {
    type Output = ();

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>
    ) -> std::task::Poll<Self::Output> {
        while let Poll::Ready(Some((pools, subscriber))) = self.recv.poll_recv(cx) {
            self.handle_new_subscription(pools, subscriber);
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

                self.seq_id = 0;
            }

            // inc seq_id
            let seq_id = self.seq_id;
            // given that we have a max update speed of 10ms, the max
            // this should reach is 1200 before a new block update
            // occurs. Becuase of this, there is no need to check for overflow
            // as 65535 is more than enough
            self.seq_id += 1;

            let orders = self.orders.get_all_orders();
            self.spawn_book_solvers(seq_id, orders);
        }

        Poll::Pending
    }
}
