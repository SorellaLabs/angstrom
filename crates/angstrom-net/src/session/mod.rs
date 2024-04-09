use tokio::sync::mpsc;

pub mod handle;
pub use handle::*;

pub mod protocol_handler;
pub use protocol_handler::*;

pub mod session;
use futures::Stream;
pub use session::*;
pub mod config;
pub use config::*;
use futures::task::Context;
pub mod connection_handler;
use std::{
    collections::HashMap,
    fmt::Debug,
    net::SocketAddr,
    pin::Pin,
    sync::{atomic::AtomicU64, Arc}
};

pub use connection_handler::*;
use futures::task::Poll;
use reth_eth_wire::DisconnectReason;
use reth_network::Direction;
use reth_primitives::PeerId;
use tracing::warn;

use crate::{errors::StromStreamError, PeerKind, StromMessage, StromProtocolMessage};

#[derive(Debug)]
pub struct StromSessionManager {
    // All active sessions that are ready to exchange messages.
    active_sessions: HashMap<PeerId, StromSessionHandle>,

    /// Channel to receive the session handle upon initialization from the
    /// connection handler This channel is also used to receive messages
    /// from the session
    from_sessions: mpsc::Receiver<StromSessionMessage>
}

impl StromSessionManager {
    pub fn new(from_sessions: mpsc::Receiver<StromSessionMessage>) -> Self {
        Self { from_sessions, active_sessions: HashMap::default() }
    }

    /// Sends a message to the peer's session
    pub fn send_message(&mut self, peer_id: &PeerId, msg: StromMessage) {
        if let Some(session) = self.active_sessions.get_mut(peer_id) {
            let _ = session
                .commands_to_session
                .try_send(SessionCommand::Message(msg));
        }
    }

    pub fn broadcast_message(&mut self, msg: StromMessage) {
        self.active_sessions.values_mut().for_each(|cmd| {
            let _ = cmd
                .commands_to_session
                .try_send(SessionCommand::Message(msg.clone()));
        })
    }

    // Removes the Session handle if it exists.
    fn remove_session(&mut self, id: &PeerId) -> Option<StromSessionHandle> {
        let session = self.active_sessions.remove(id)?;
        Some(session)
    }

    /// Shutdown all active sessions.
    pub fn disconnect_all(&self, reason: Option<DisconnectReason>) {
        for (_, session) in self.active_sessions.iter() {
            session.disconnect(reason);
        }
    }

    pub fn disconnect(&mut self, id: PeerId, reason: Option<DisconnectReason>) {
        if let Some(session) = self.active_sessions.remove(&id) {
            session.disconnect(reason)
        }
    }

    fn poll_session_msg(&mut self, cx: &mut Context<'_>) -> Poll<Option<SessionEvent>> {
        self.from_sessions.poll_recv(cx).map(|msg| {
            tracing::debug!(?msg, "got msg from session");
            msg.and_then(|msg_inner| match msg_inner {
                StromSessionMessage::Disconnected { peer_id } => {
                    self.remove_session(&peer_id);
                    Some(SessionEvent::Disconnected { peer_id })
                }
                StromSessionMessage::Established { handle } => {
                    if self.active_sessions.contains_key(&handle.remote_id) {
                        warn!(peer_id=?handle.remote_id, "got duplicate connection");
                        // disconnect
                        handle.disconnect(None);

                        return None
                    }

                    let event = SessionEvent::SessionEstablished {
                        peer_id:   handle.remote_id,
                        direction: handle.direction,
                        timeout:   Arc::new(AtomicU64::new(40))
                    };
                    self.active_sessions.insert(handle.remote_id, handle);

                    Some(event)
                }
                StromSessionMessage::ClosedOnConnectionError { peer_id, error } => {
                    Some(SessionEvent::OutgoingConnectionError { peer_id, error })
                }
                StromSessionMessage::ValidMessage { peer_id, message } => {
                    Some(SessionEvent::ValidMessage { peer_id, message })
                }
                StromSessionMessage::BadMessage { peer_id } => {
                    Some(SessionEvent::BadMessage { peer_id })
                }
                StromSessionMessage::ProtocolBreach { peer_id } => {
                    Some(SessionEvent::ProtocolBreach { peer_id })
                }
            })
        })
    }
}

impl Stream for StromSessionManager {
    type Item = SessionEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<SessionEvent>> {
        self.poll_session_msg(cx)
    }
}

/// Events produced by the [`SessionManager`]
#[derive(Debug)]
pub enum SessionEvent {
    /// A new session was successfully authenticated.
    ///
    /// This session is now able to exchange data.
    SessionEstablished {
        /// The remote node's public key
        peer_id:   PeerId,
        /// The direction of the session, either `Inbound` or `Outgoing`
        direction: Direction,
        /// The maximum time that the session waits for a response from the peer
        /// before timing out the connection
        timeout:   Arc<AtomicU64>
    },
    /// The peer was already connected with another session.
    AlreadyConnected {
        /// The remote node's public key
        peer_id:     PeerId,
        /// The remote node's socket address
        remote_addr: SocketAddr,
        /// The direction of the session, either `Inbound` or `Outgoing`
        direction:   Direction
    },
    /// A session received a valid message via RLPx.
    ValidMessage {
        /// The remote node's public key
        peer_id: PeerId,
        /// Message received from the peer.
        message: StromProtocolMessage
    },
    /// Received a bad message from the peer.
    BadMessage {
        /// Identifier of the remote peer.
        peer_id: PeerId
    },
    /// Remote peer is considered in protocol violation
    ProtocolBreach {
        /// Identifier of the remote peer.
        peer_id: PeerId
    },

    /// Failed to establish a tcp stream
    OutgoingConnectionError {
        /// The remote node's public key
        peer_id: PeerId,
        /// The error that caused the outgoing connection to fail
        error:   StromStreamError
    },
    /// Session was closed due to an error
    SessionClosedOnConnectionError {
        /// The id of the remote peer.
        peer_id:     PeerId,
        /// The socket we were connected to.
        remote_addr: SocketAddr,
        /// The error that caused the session to close
        error:       StromStreamError
    },
    /// Active session was gracefully disconnected.
    Disconnected {
        /// The remote node's public key
        peer_id: PeerId
    }
}
