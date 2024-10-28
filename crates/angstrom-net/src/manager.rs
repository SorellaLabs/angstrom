use std::{
    future::Future,
    pin::Pin,
    sync::{atomic::AtomicUsize, Arc},
    task::{Context, Poll}
};

use alloy::primitives::BlockNumber;
use alloy_rpc_types::Block;
use angstrom_types::{
    consensus::{PreProposal, Proposal},
    primitive::PeerId,
    sol_bindings::ext::RawPoolOrder
};
use futures::StreamExt;
use reth_eth_wire::DisconnectReason;
use reth_metrics::common::mpsc::UnboundedMeteredSender;
use tokio::sync::mpsc::UnboundedSender;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::error;

use crate::{NetworkOrderEvent, StromMessage, StromNetworkHandleMsg, Swarm, SwarmEvent};
#[allow(unused_imports)]
use crate::{StromNetworkConfig, StromNetworkHandle, StromSessionManager};

#[allow(dead_code)]
pub struct StromNetworkManager<DB> {
    handle: StromNetworkHandle,

    from_handle_rx:       UnboundedReceiverStream<StromNetworkHandleMsg>,
    to_pool_manager:      Option<UnboundedMeteredSender<NetworkOrderEvent>>,
    to_consensus_manager: Option<UnboundedMeteredSender<StromConsensusEvent>>,

    event_listeners:  Vec<UnboundedSender<StromNetworkEvent>>,
    swarm:            Swarm<DB>,
    /// This is updated via internal events and shared via `Arc` with the
    /// [`NetworkHandle`] Updated by the `NetworkWorker` and loaded by the
    /// `NetworkService`.
    num_active_peers: Arc<AtomicUsize>
}

impl<DB: Unpin> StromNetworkManager<DB> {
    pub fn new(
        swarm: Swarm<DB>,
        to_pool_manager: Option<UnboundedMeteredSender<NetworkOrderEvent>>,
        to_consensus_manager: Option<UnboundedMeteredSender<StromConsensusEvent>>
    ) -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        let peers = Arc::new(AtomicUsize::default());
        let handle =
            StromNetworkHandle::new(peers.clone(), UnboundedMeteredSender::new(tx, "strom handle"));

        Self {
            handle: handle.clone(),
            num_active_peers: peers,
            swarm,
            from_handle_rx: rx.into(),
            to_pool_manager,
            to_consensus_manager,
            event_listeners: Vec::new()
        }
    }

    pub fn install_pool_manager(&mut self, tx: UnboundedMeteredSender<NetworkOrderEvent>) {
        self.to_pool_manager = Some(tx);
    }

    pub fn install_consensus_manager(&mut self, tx: UnboundedMeteredSender<StromConsensusEvent>) {
        self.to_consensus_manager = Some(tx);
    }

    pub fn remove_consensus_manager(&mut self) {
        self.to_consensus_manager.take();
    }

    pub fn remove_pool_manager(&mut self) {
        self.to_pool_manager.take();
    }

    pub fn swarm_mut(&mut self) -> &mut Swarm<DB> {
        &mut self.swarm
    }

    pub fn swarm(&self) -> &Swarm<DB> {
        &self.swarm
    }

    pub fn get_handle(&self) -> StromNetworkHandle {
        self.handle.clone()
    }

    // Handler for received messages from a handle
    fn on_handle_message(&mut self, msg: StromNetworkHandleMsg) {
        match msg {
            StromNetworkHandleMsg::SubscribeEvents(tx) => self.event_listeners.push(tx),
            StromNetworkHandleMsg::SendStromMessage { peer_id, msg } => {
                self.swarm.sessions_mut().send_message(&peer_id, msg)
            }
            StromNetworkHandleMsg::Shutdown(tx) => {
                // Disconnect all active connections
                self.swarm
                    .sessions_mut()
                    .disconnect_all(Some(DisconnectReason::ClientQuitting));

                // drop pending connections

                let _ = tx.send(());
            }
            StromNetworkHandleMsg::RemovePeer(peer_id) => {
                self.swarm.state_mut().peers_mut().remove_peer(peer_id);
            }
            StromNetworkHandleMsg::ReputationChange(peer_id, kind) => self
                .swarm
                .state_mut()
                .peers_mut()
                .change_weight(peer_id, kind),
            StromNetworkHandleMsg::BroadcastStromMessage { msg } => {
                self.swarm_mut().sessions_mut().broadcast_message(msg);
            }
            StromNetworkHandleMsg::DisconnectPeer(id, reason) => {
                self.swarm_mut().sessions_mut().disconnect(id, reason);
            }
        }
    }

    fn notify_listeners(&mut self, event: StromNetworkEvent) {
        self.event_listeners
            .retain(|tx| tx.send(event.clone()).is_ok());
    }
}

impl<DB: Unpin> Future for StromNetworkManager<DB> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // process incoming messages from a handle
        let mut work = 30;
        loop {
            work -= 1;
            if work == 0 {
                cx.waker().wake_by_ref();
                return Poll::Pending
            }

            match self.from_handle_rx.poll_next_unpin(cx) {
                Poll::Ready(Some(msg)) => self.on_handle_message(msg),
                Poll::Ready(None) => {
                    // This is only possible if the channel was deliberately closed since we always
                    // have an instance of `NetworkHandle`
                    error!("Strom network message channel closed.");
                    return Poll::Ready(())
                }
                _ => {}
            };

            if let Poll::Ready(Some(event)) = self.swarm.poll_next_unpin(cx) {
                match event {
                    SwarmEvent::ValidMessage { peer_id, msg } => match msg {
                        StromMessage::PrePropose(p) => {
                            self.to_consensus_manager.as_ref().inspect(|tx| {
                                tx.send(StromConsensusEvent::PreProposal(peer_id, p));
                            });
                        }
                        StromMessage::Propose(a) => {
                            self.to_consensus_manager.as_ref().inspect(|tx| {
                                tx.send(StromConsensusEvent::Proposal(peer_id, a));
                            });
                        }
                        StromMessage::PropagatePooledOrders(a) => {
                            self.to_pool_manager.as_ref().inspect(|tx| {
                                tx.send(NetworkOrderEvent::IncomingOrders { peer_id, orders: a });
                            });
                        }
                        _ => {}
                    },
                    SwarmEvent::Disconnected { peer_id } => {
                        self.notify_listeners(StromNetworkEvent::SessionClosed {
                            peer_id,
                            reason: None
                        })
                    }
                    SwarmEvent::SessionEstablished { peer_id } => {
                        self.num_active_peers
                            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        self.notify_listeners(StromNetworkEvent::SessionEstablished { peer_id })
                    }
                }
            }
        }

        Poll::Pending
    }
}

/// (Non-exhaustive) Events emitted by the network that are of interest for
/// subscribers.
///
/// This includes any event types that may be relevant to tasks, for metrics,
/// keep track of peers etc.
#[derive(Debug, Clone)]
pub enum StromNetworkEvent {
    /// Closed the peer session.
    SessionClosed {
        /// The identifier of the peer to which a session was closed.
        peer_id: PeerId,
        /// Why the disconnect was triggered
        reason:  Option<DisconnectReason>
    },
    /// Established a new session with the given peer.
    SessionEstablished {
        /// The identifier of the peer to which a session was established.
        peer_id: PeerId
    },
    /// Event emitted when a new peer is added
    PeerAdded(PeerId),
    /// Event emitted when a new peer is removed
    PeerRemoved(PeerId)
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum StromConsensusEvent {
    PreProposal(PeerId, PreProposal),
    Proposal(PeerId, Proposal)
}

impl StromConsensusEvent {
    pub fn message_type(&self) -> &'static str {
        match self {
            StromConsensusEvent::PreProposal(..) => "PreProposal",
            StromConsensusEvent::Proposal(..) => "Proposal"
        }
    }

    pub fn sender(&self) -> PeerId {
        match self {
            StromConsensusEvent::PreProposal(peer_id, _) => *peer_id,
            StromConsensusEvent::Proposal(peer_id, _) => *peer_id
        }
    }

    pub fn payload_source(&self) -> PeerId {
        match self {
            StromConsensusEvent::PreProposal(_, pre_proposal) => pre_proposal.source,
            StromConsensusEvent::Proposal(_, proposal) => proposal.source
        }
    }

    pub fn block_height(&self) -> BlockNumber {
        match self {
            StromConsensusEvent::PreProposal(_, PreProposal { block_height, .. }) => *block_height,
            StromConsensusEvent::Proposal(_, Proposal { block_height, .. }) => *block_height
        }
    }
}

impl From<StromConsensusEvent> for StromMessage {
    fn from(event: StromConsensusEvent) -> Self {
        match event {
            StromConsensusEvent::PreProposal(_, pre_proposal) => {
                StromMessage::PrePropose(pre_proposal)
            }
            StromConsensusEvent::Proposal(_, proposal) => StromMessage::Propose(proposal)
        }
    }
}
