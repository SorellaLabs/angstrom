use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc
    },
    task::{Context, Poll}
};

use alloy::{network::Network, providers::Provider, transports::Transport};
use consensus::ConsensusManager;
use futures::FutureExt;
use parking_lot::Mutex;
use tokio::task::JoinHandle;
use tracing::{span, Level};

pub(crate) struct TestnetConsensusFuture {
    consensus: Arc<Mutex<ConsensusManager>>,
    /// JoinHandle for the consensus future
    fut:       JoinHandle<()>
}

impl TestnetConsensusFuture {
    pub(crate) fn new(
        testnet_node_id: u64,
        consensus: ConsensusManager,
        running: Arc<AtomicBool>
    ) -> Self {
        let consensus = Arc::new(Mutex::new(consensus));
        let internal =
            TestnetConsensusFutureInternals::new(testnet_node_id, consensus.clone(), running);
        Self { consensus, fut: tokio::spawn(internal) }
    }

    pub(crate) fn consensus_manager<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&ConsensusManager) -> R
    {
        f(&self.consensus.lock())
    }

    pub(crate) fn consensus_manager_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut ConsensusManager) -> R
    {
        f(&mut self.consensus.lock())
    }
}

struct TestnetConsensusFutureInternals {
    testnet_node_id: u64,
    consensus:       Arc<Mutex<ConsensusManager>>,
    running:         Arc<AtomicBool>
}

impl TestnetConsensusFutureInternals {
    fn new(
        testnet_node_id: u64,
        consensus: Arc<Mutex<ConsensusManager>>,
        running: Arc<AtomicBool>
    ) -> Self {
        Self { testnet_node_id, consensus, running }
    }
}

impl Future for TestnetConsensusFutureInternals {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        let span = span!(Level::TRACE, "node", id = this.testnet_node_id);
        let e = span.enter();

        if this.running.load(Ordering::Relaxed) {
            {
                let mut cons = this.consensus.lock_arc();
                if cons.poll_unpin(cx).is_ready() {
                    return Poll::Ready(())
                }
            }
        }

        drop(e);

        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

impl Drop for TestnetConsensusFuture {
    fn drop(&mut self) {
        self.fut.abort();
    }
}
