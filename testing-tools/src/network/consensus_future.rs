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
use matching_engine::MatchingEngineHandle;
use parking_lot::Mutex;
use tokio::task::JoinHandle;
use tracing::{span, Level};

use crate::types::MockBlockSync;

pub(crate) struct TestnetConsensusFuture<T, Matching> {
    _consensus: Arc<Mutex<ConsensusManager<T, Matching, MockBlockSync>>>,
    /// JoinHandle for the _consensus future
    fut:        JoinHandle<()>
}

impl<T, Matching> TestnetConsensusFuture<T, Matching>
where
    T: Transport + Clone,
    Matching: MatchingEngineHandle
{
    pub(crate) fn new(
        testnet_node_id: u64,
        _consensus: ConsensusManager<T, Matching, MockBlockSync>,
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
        F: FnOnce(&ConsensusManager<T, Matching, MockBlockSync>) -> R
    {
        f(&self._consensus.lock())
    }

    #[allow(dead_code)]
    pub(crate) fn consensus_manager_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut ConsensusManager<T, Matching, MockBlockSync>) -> R
    {
        f(&mut self._consensus.lock())
    }
}

struct TestnetConsensusFutureInternals<T, Matching> {
    testnet_node_id: u64,
    _consensus:      Arc<Mutex<ConsensusManager<T, Matching, MockBlockSync>>>,
    running:         Arc<AtomicBool>
}

impl<T, Matching> TestnetConsensusFutureInternals<T, Matching>
where
    T: Transport,
    Matching: MatchingEngineHandle
{
    fn new(
        testnet_node_id: u64,
        _consensus: Arc<Mutex<ConsensusManager<T, Matching, MockBlockSync>>>,
        running: Arc<AtomicBool>
    ) -> Self {
        Self { testnet_node_id, _consensus, running }
    }
}

impl<T, Matching> Future for TestnetConsensusFutureInternals<T, Matching>
where
    T: Transport + Clone,
    Matching: MatchingEngineHandle
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

impl<T, Matching> Drop for TestnetConsensusFuture<T, Matching> {
    fn drop(&mut self) {
        self.fut.abort();
    }
}
