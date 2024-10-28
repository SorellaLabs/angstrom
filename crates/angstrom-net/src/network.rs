use std::sync::{atomic::AtomicUsize, Arc};

use angstrom_types::{primitive::PeerId, sol_bindings::grouped_orders::AllOrders};
use reth_metrics::common::mpsc::UnboundedMeteredSender;
use reth_network::DisconnectReason;
use tokio::sync::{
    mpsc::{unbounded_channel, UnboundedSender},
    oneshot
};
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::{ReputationChangeKind, StromMessage, StromNetworkEvent};

//TODO:
// 1) Implement the order pool manager
// 2) Implement the consensus manager
// 3)
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct StromNetworkHandle {
    inner: Arc<StromNetworkInner>
}

impl StromNetworkHandle {
    pub fn new(
        num_active_peers: Arc<AtomicUsize>,
        to_manager_tx: UnboundedMeteredSender<StromNetworkHandleMsg>
    ) -> Self {
        Self { inner: Arc::new(StromNetworkInner { num_active_peers, to_manager_tx }) }
    }

    /// Sends a [`NetworkHandleMessage`] to the manager
    fn send_to_network_manager(&self, msg: StromNetworkHandleMsg) {
        let _ = self.inner.to_manager_tx.send(msg);
    }

    /// Send Strom message to peer
    pub fn send_message(&self, peer_id: PeerId, msg: StromMessage) {
        tracing::debug!("sent message to peer {:?}", peer_id);
        panic!("sent message to peer {:?}", peer_id);
        self.send_to_network_manager(StromNetworkHandleMsg::SendStromMessage { peer_id, msg })
    }

    /// Broadcast Strom message to all peers
    pub fn broadcast_message(&self, msg: StromMessage) {
        self.send_to_network_manager(StromNetworkHandleMsg::BroadcastStromMessage { msg })
    }

    pub fn peer_reputation_change(&self, peer: PeerId, change: ReputationChangeKind) {
        self.send_to_network_manager(StromNetworkHandleMsg::ReputationChange(peer, change));
    }

    pub fn subscribe_network_events(&self) -> UnboundedReceiverStream<StromNetworkEvent> {
        let (tx, rx) = unbounded_channel();
        self.send_to_network_manager(StromNetworkHandleMsg::SubscribeEvents(tx));

        UnboundedReceiverStream::from(rx)
    }

    /// Send message to gracefully shutdown node.
    ///
    /// This will disconnect all active and pending sessions and prevent
    /// new connections to be established.
    pub async fn shutdown(&self) -> Result<(), oneshot::error::RecvError> {
        let (tx, rx) = oneshot::channel();
        self.send_to_network_manager(StromNetworkHandleMsg::Shutdown(tx));
        rx.await
    }

    /// Sends a message to the [`NetworkManager`](crate::NetworkManager) to
    /// remove a peer from the set corresponding to given kind.
    pub fn remove_peer(&self, peer: PeerId) {
        self.send_to_network_manager(StromNetworkHandleMsg::RemovePeer(peer))
    }

    pub fn peer_count(&self) -> usize {
        self.inner
            .num_active_peers
            .load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[derive(Debug)]
#[allow(dead_code)]
struct StromNetworkInner {
    num_active_peers: Arc<AtomicUsize>,

    to_manager_tx: UnboundedMeteredSender<StromNetworkHandleMsg>
}

/// All events related to orders emitted by the network.
#[derive(Debug, Clone, PartialEq)]
pub enum NetworkOrderEvent {
    IncomingOrders { peer_id: PeerId, orders: Vec<AllOrders> }
}

#[derive(Debug)]
pub enum StromNetworkHandleMsg {
    SubscribeEvents(UnboundedSender<StromNetworkEvent>),
    /// Removes a peer from the peer set corresponding to the given kind.
    RemovePeer(PeerId),
    /// Disconnect a connection to a peer if it exists.
    DisconnectPeer(PeerId, Option<DisconnectReason>),

    /// Sends the strom message to a single peer.
    SendStromMessage {
        peer_id: PeerId,
        msg:     StromMessage
    },

    /// Broadcasts the storm message to all peers
    BroadcastStromMessage {
        msg: StromMessage
    },

    /// Apply a reputation change to the given peer.
    ReputationChange(PeerId, ReputationChangeKind),
    /// Gracefully shutdown network
    Shutdown(oneshot::Sender<()>)
}
