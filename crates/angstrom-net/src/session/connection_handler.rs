use std::{collections::HashSet, net::SocketAddr, pin::Pin};

use alloy::{
    primitives::{keccak256, Address},
    rlp::BytesMut,
};
use angstrom_types::primitive::PeerId;
use futures::{stream::Empty, Stream, StreamExt};
use reth_eth_wire::{
    capability::SharedCapabilities, multiplex::ProtocolConnection, protocol::Protocol,
    DisconnectReason,
};
use reth_metrics::common::mpsc::MeteredPollSender;
use reth_network::{
    protocol::{ConnectionHandler, OnNotSupported},
    Direction,
};
use tokio::{
    sync::mpsc,
    time::{Duration, Instant},
};
use tokio_stream::wrappers::ReceiverStream;

use crate::{
    errors::StromStreamError,
    session::handle::StromSessionHandle,
    types::message::{StromMessage, StromProtocolMessage},
    StromSession, VerificationSidecar,
};

pub enum PossibleStromSession {
    Session(StromSession),
    /// this will instantly terminate when first polled
    Invalid(Empty<BytesMut>),
}

impl Stream for PossibleStromSession {
    type Item = BytesMut;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        match *self {
            PossibleStromSession::Session(ref mut s) => s.poll_next_unpin(cx),
            PossibleStromSession::Invalid(ref mut i) => i.poll_next_unpin(cx),
        }
    }
}

//TODO: Add bandwith meter to be
pub struct StromConnectionHandler {
    pub to_session_manager: MeteredPollSender<StromSessionMessage>,
    pub protocol_breach_request_timeout: Duration,
    pub session_command_buffer: usize,
    pub socket_addr: SocketAddr,
    pub side_car: VerificationSidecar,
    pub validator_set: HashSet<Address>,
}

impl ConnectionHandler for StromConnectionHandler {
    type Connection = PossibleStromSession;

    fn protocol(&self) -> Protocol {
        StromProtocolMessage::protocol()
    }

    fn on_unsupported_by_peer(
        self,
        _supported: &SharedCapabilities,
        _direction: Direction,
        _peer_id: PeerId,
    ) -> OnNotSupported {
        OnNotSupported::Disconnect
    }

    // this occurs after the eth handshake occured
    fn into_connection(
        self,
        direction: Direction,
        peer_id: PeerId,
        conn: ProtocolConnection,
    ) -> Self::Connection {
        let hash = keccak256(peer_id);
        let validator_address = Address::from_slice(&hash[12..]);
        if !self.validator_set.contains(&validator_address) {
            return PossibleStromSession::Invalid(futures::stream::empty());
        }

        let (tx, rx) = mpsc::channel(self.session_command_buffer);

        let handle = StromSessionHandle {
            direction,
            remote_id: peer_id,
            established: Instant::now(),
            commands_to_session: tx,
        };

        PossibleStromSession::Session(StromSession::new(
            conn,
            peer_id,
            ReceiverStream::new(rx),
            self.to_session_manager,
            self.protocol_breach_request_timeout,
            self.side_car,
            handle,
        ))
    }
}

/// Message variants an active session can produce and send back to the
/// [`SessionManager`](crate::session::SessionManager)
#[derive(Debug)]
pub enum StromSessionMessage {
    /// Session was established.
    Established { handle: StromSessionHandle },

    /// Session was gracefully disconnected.
    Disconnected {
        /// The remote node's public key
        peer_id: PeerId,
    },
    /// Session was closed due an error
    ClosedOnConnectionError {
        /// The remote node's public key
        peer_id: PeerId,

        /// The error that caused the session to close
        error: StromStreamError,
    },
    /// A session received a valid message via RLPx.
    ValidMessage {
        /// Identifier of the remote peer.
        peer_id: PeerId,
        /// Message received from the peer.
        message: StromProtocolMessage,
    },
    /// Received a bad message from the peer.
    BadMessage {
        /// Identifier of the remote peer.
        peer_id: PeerId,
    },
    /// Remote peer is considered in protocol violation
    ProtocolBreach {
        /// Identifier of the remote peer.
        peer_id: PeerId,
    },
}

#[derive(Debug)]
pub enum StromSessionCommand {
    /// Disconnect the connection
    Disconnect {
        /// Why the disconnect was initiated
        reason: Option<DisconnectReason>,
    },
    /// Sends a message to the peer
    Message(StromMessage),
}
