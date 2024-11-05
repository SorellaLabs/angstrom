use std::{
    future::Future,
    ops::Drop,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc
    },
    task::{Context, Poll}
};

use alloy::transports::Transport;
use consensus::ConsensusManager;
use futures::FutureExt;
use parking_lot::Mutex;
use tokio::task::JoinHandle;
use tracing::{span, Level};

pub(crate) struct TestnetConsensusFuture<T> {
    _consensus: Arc<Mutex<ConsensusManager<T>>>,
    /// JoinHandle for the _consensus future
    fut:        JoinHandle<()>
}

impl<T> TestnetConsensusFuture<T>
where
    T: Transport + Clone
{
    pub(crate) fn new(
        testnet_node_id: u64,
        _consensus: ConsensusManager<T>,
        running: Arc<AtomicBool>
    ) -> Self {
        let _consensus = Arc::new(Mutex::new(_consensus));
        let internal =
            TestnetConsensusFutureInternals::new(testnet_node_id, _consensus.clone(), running);
        Self { _consensus, fut: tokio::spawn(internal) }
    }

    #[allow(dead_code)]
    pub(crate) fn consensus_manager<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&ConsensusManager<T>) -> R
    {
        f(&self._consensus.lock())
    }

    #[allow(dead_code)]
    pub(crate) fn consensus_manager_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut ConsensusManager<T>) -> R
    {
        f(&mut self._consensus.lock())
    }
}

struct TestnetConsensusFutureInternals<T> {
    testnet_node_id: u64,
    _consensus:      Arc<Mutex<ConsensusManager<T>>>,
    running:         Arc<AtomicBool>
}

impl<T> TestnetConsensusFutureInternals<T>
where
    T: Transport
{
    fn new(
        testnet_node_id: u64,
        _consensus: Arc<Mutex<ConsensusManager<T>>>,
        running: Arc<AtomicBool>
    ) -> Self {
        Self { testnet_node_id, _consensus, running }
    }
}

impl<T> Future for TestnetConsensusFutureInternals<T>
where
    T: Transport + Clone
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        let span = span!(Level::TRACE, "node", id = this.testnet_node_id);
        let e = span.enter();

        if this.running.load(Ordering::Relaxed) {
            {
                let mut cons = this._consensus.lock_arc();
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

impl<T> Drop for TestnetConsensusFuture<T> {
    fn drop(&mut self) {
        self.fut.abort();
    }
}
