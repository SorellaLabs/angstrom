use std::{
    future::Future,
    pin::Pin,
    sync::{atomic::AtomicUsize, Arc},
    task::Context
};

use futures::{task::Poll, StreamExt};
use guard_types::consensus::{Commit, PreProposal, Proposal};
use reth_eth_wire::DisconnectReason;
use reth_metrics::common::mpsc::UnboundedMeteredSender;
use reth_primitives::PeerId;
use tokio::sync::mpsc::UnboundedSender;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::error;

use crate::{NetworkOrderEvent, StromMessage, StromNetworkHandleMsg, Swarm, SwarmEvent};
#[allow(unused_imports)]
use crate::{StromNetworkConfig, StromNetworkHandle, StromSessionManager};

#[allow(dead_code)]
pub struct StromNetworkManager {
    handle:               StromNetworkHandle,
    from_handle_rx:       UnboundedReceiverStream<StromNetworkHandleMsg>,
    to_pool_manager:      Option<UnboundedMeteredSender<NetworkOrderEvent>>,
    to_consensus_manager: Option<UnboundedMeteredSender<StromConsensusEvent>>,
    subscriptions:        Vec<UnboundedSender<UnboundedSender<StromNetworkEvent>>>,

    swarm:            Swarm,
    /// This is updated via internal events and shared via `Arc` with the
    /// [`NetworkHandle`] Updated by the `NetworkWorker` and loaded by the
    /// `NetworkService`.
    num_active_peers: Arc<AtomicUsize>
}

impl StromNetworkManager {
    // Handler for received messages from a handle
    fn on_handle_message(&mut self, msg: StromNetworkHandleMsg) {
        match msg {
            StromNetworkHandleMsg::SendOrders { peer_id, msg } => {
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
                self.swarm.state_mut().remove_peer(peer_id);
            }
            StromNetworkHandleMsg::ReputationChange(peer_id, kind) => {
                self.swarm.state_mut().change_weight(peer_id, kind)
            }
            _ => {
                todo!()
            }
        }
    }

    /// Sends an event to the pool manager.
    fn notify_pool_manager(&self, event: NetworkOrderEvent) {
        if let Some(ref tx) = self.to_pool_manager {
            let _ = tx.send(event);
        }
    }
}

impl Future for StromNetworkManager {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // process incoming messages from a handle
        loop {
            match self.from_handle_rx.poll_next_unpin(cx) {
                Poll::Pending => break,
                Poll::Ready(None) => {
                    // This is only possible if the channel was deliberately closed since we always
                    // have an instance of `NetworkHandle`
                    error!("Strom network message channel closed.");
                    return Poll::Ready(())
                }
                Poll::Ready(Some(msg)) => self.on_handle_message(msg)
            };

            macro_rules! send_msgs {
                ($name:ident, $peer_id:ident, $($var:ident),+) => {
                    match $name {
                        $(
                            StromMessage::$var(a) => {
                                self.to_consensus_manager
                                    .as_mut()
                                    .map(|tx| tx.send(StromConsensusEvent::$var($peer_id, a)));
                            },
                        )+
                        StromMessage::PropagatePooledOrders(a) => {
                            self.to_pool_manager
                                .as_mut()
                                .map(|tx| tx.send(NetworkOrderEvent::IncomingOrders {
                                    $peer_id,
                                    orders: a
                                }));
                        },
                        _ => {}
                    }
                };
            }

            if let Poll::Ready(Some(event)) = self.swarm.poll_next_unpin(cx) {
                match event {
                    SwarmEvent::ValidMessage { peer_id, msg } => {
                        send_msgs!(msg, peer_id, Commit, Propose, PrePropose)
                    }
                    // StromMessage::Commit(c) => self
                    //     .to_consensus_manager
                    //     .as_mut()
                    //     .map(|tx| tx.send(StromConsensusEvent::Commit(peer_id, c))),
                    // StromMessage::Propose(p) => self
                    //     .to_consensus_manager
                    //     .as_mut()
                    //     .map(|tx| tx.send(StromConsensusEvent::Commit(peer_id, c)))
                    SwarmEvent::Disconnected { peer_id } => {}
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
        peer_id:        PeerId,
        /// The client version of the peer to which a session was established.
        client_version: Arc<str>
    },
    /// Event emitted when a new peer is added
    PeerAdded(PeerId),
    /// Event emitted when a new peer is removed
    PeerRemoved(PeerId)
}

#[derive(Debug, Clone)]
pub enum StromConsensusEvent {
    PrePropose(PeerId, PreProposal),
    Propose(PeerId, Proposal),
    Commit(PeerId, Commit)
}
