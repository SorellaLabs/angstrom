use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc
    },
    task::{Context, Poll}
};

use angstrom_network::StromNetworkManager;
use futures::FutureExt;
use parking_lot::Mutex;
use reth_network::test_utils::Peer;
use reth_provider::BlockReader;
use tokio::task::JoinHandle;
use tracing::{span, Level};

pub(crate) struct TestnetPeerStateFuture<C> {
    eth_peer:      Arc<Mutex<Peer<C>>>,
    /// the default ethereum network peer
    strom_network: Arc<Mutex<StromNetworkManager<C>>>,
    /// JoinHandle for the network futures
    fut:           JoinHandle<()>
}

impl<C> TestnetPeerStateFuture<C>
where
    C: Unpin + BlockReader + 'static
{
    pub(crate) fn new(
        testnet_node_id: u64,
        eth_peer: Peer<C>,
        strom_network: StromNetworkManager<C>,
        running: Arc<AtomicBool>
    ) -> Self {
        let eth_peer = Arc::new(Mutex::new(eth_peer));
        let strom_network = Arc::new(Mutex::new(strom_network));
        let internal = TestnetPeerStateFutureInternals::new(
            testnet_node_id,
            eth_peer.clone(),
            strom_network.clone(),
            running
        );
        Self { eth_peer, strom_network, fut: tokio::spawn(internal) }
    }

    pub(crate) fn strom_network<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&StromNetworkManager<C>) -> R
    {
        f(&self.strom_network.lock())
    }

    pub(crate) fn strom_network_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut StromNetworkManager<C>) -> R
    {
        f(&mut self.strom_network.lock())
    }

    pub(crate) fn eth_peer<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Peer<C>) -> R
    {
        f(&self.eth_peer.lock())
    }

    pub(crate) fn eth_peer_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Peer<C>) -> R
    {
        f(&mut self.eth_peer.lock())
    }

    pub(crate) fn poll_fut_to_initialize(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        self.fut.poll_unpin(cx).map(|_| ())
    }
}

struct TestnetPeerStateFutureInternals<C> {
    testnet_node_id:   u64,
    /// the default ethereum network peer
    eth_peer_fut:      Arc<Mutex<Peer<C>>>,
    strom_network_fut: Arc<Mutex<StromNetworkManager<C>>>,
    running:           Arc<AtomicBool>
}

impl<C> TestnetPeerStateFutureInternals<C> {
    fn new(
        testnet_node_id: u64,
        eth_peer_fut: Arc<Mutex<Peer<C>>>,
        strom_network_fut: Arc<Mutex<StromNetworkManager<C>>>,
        running: Arc<AtomicBool>
    ) -> Self {
        Self { testnet_node_id, eth_peer_fut, strom_network_fut, running }
    }
}

impl<C> Future for TestnetPeerStateFutureInternals<C>
where
    C: Unpin + BlockReader + 'static
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        let span = span!(Level::TRACE, "node", id = this.testnet_node_id);
        let e = span.enter();

        if this.running.load(Ordering::Relaxed) {
            {
                let mut eth = this.eth_peer_fut.lock_arc();
                if eth.poll_unpin(cx).is_ready() {
                    return Poll::Ready(())
                }
            }

            {
                let mut strom = this.strom_network_fut.lock_arc();
                if strom.poll_unpin(cx).is_ready() {
                    return Poll::Ready(())
                }
            }
        }

        drop(e);

        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

impl<C> Drop for TestnetPeerStateFuture<C> {
    fn drop(&mut self) {
        self.fut.abort();
    }
}
