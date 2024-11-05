use std::{
    collections::VecDeque,
    fmt::Debug,
    ops::Deref,
    pin::Pin,
    time::{SystemTime, UNIX_EPOCH}
};

use alloy::rlp::{BytesMut, Encodable};
use angstrom_types::primitive::PeerId;
use angstrom_utils::{GenericExt, PollFlatten};
use futures::{
    task::{Context, Poll},
    Stream, StreamExt
};
use reth_eth_wire::multiplex::ProtocolConnection;
use reth_metrics::common::mpsc::MeteredPollSender;
use secp256k1::SecretKey;
use tokio::time::Duration;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::PollSender;

use super::handle::SessionCommand;
use crate::{
    types::{
        message::StromProtocolMessage,
        status::{Status, StatusState}
    },
    StatusBuilder, StromMessage, StromSessionHandle, StromSessionMessage
};

const STATUS_TIMESTAMP_TIMEOUT_MS: u128 = 1500;

/// holds the state we need to verify the new peer
#[derive(Clone)]
pub struct VerificationSidecar {
    pub secret_key:   SecretKey,
    pub status:       StatusState,
    pub has_sent:     bool,
    pub has_received: bool
}

impl VerificationSidecar {
    pub fn make_status_message(&mut self, peer: PeerId) -> Status {
        if self.has_sent {
            panic!("can only send the status message once");
        }

        StatusBuilder::from(self.status.with_peer(peer)).build(self.secret_key)
    }

    pub fn is_verified(&self) -> bool {
        self.has_sent && self.has_received
    }
}

impl Debug for VerificationSidecar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("status: {:?}", self.status))
    }
}

#[allow(dead_code)]
pub struct StromSession {
    /// The underlying connection.
    pub(crate) conn:               ProtocolConnection,
    /// Identifier of the node we're connected to.
    pub(crate) remote_peer_id:     PeerId,
    /// Incoming commands from the manager
    pub(crate) commands_rx:        ReceiverStream<SessionCommand>,
    /// Sink to send messages to the [`SessionManager`](super::SessionManager).
    pub(crate) to_session_manager: MeteredPollSender<StromSessionMessage>,

    /// If an [ActiveSession] does not receive a response at all within this
    /// duration then it is considered a protocol violation and the session
    /// will initiate a drop.
    pub(crate) protocol_breach_request_timeout: Duration,
    /// Used to reserve a slot to guarantee that the termination message is
    /// delivered
    pub(crate) terminate_message: Option<(PollSender<StromSessionMessage>, StromSessionMessage)>,
    /// has a value until verification has been completed.
    pub verification_sidecar: VerificationSidecar,
    /// has sent the handle to the receiver
    pending_handle: Option<StromSessionHandle>,
    /// buffer for pending messages
    outbound_buffer: VecDeque<StromSessionMessage>
}

impl StromSession {
    pub fn new(
        conn: ProtocolConnection,
        peer_id: PeerId,
        commands_rx: ReceiverStream<SessionCommand>,
        to_session_manager: MeteredPollSender<StromSessionMessage>,
        protocol_breach_request_timeout: Duration,
        verification_sidecar: VerificationSidecar,
        handle: StromSessionHandle
    ) -> Self {
        Self {
            verification_sidecar,
            conn,
            remote_peer_id: peer_id,
            commands_rx,
            to_session_manager,
            protocol_breach_request_timeout,
            terminate_message: None,
            pending_handle: Some(handle),
            outbound_buffer: VecDeque::default()
        }
    }

    /// Report back that this session has been closed.
    fn emit_disconnect(&mut self, cx: &mut Context<'_>) -> Poll<Option<BytesMut>> {
        let msg = StromSessionMessage::Disconnected { peer_id: self.remote_peer_id };

        self.terminate_message = Some((self.to_session_manager.inner().clone(), msg));
        self.poll_terminate_message(cx).expect("message is set")
    }

    fn poll_init_connection(&mut self, cx: &mut Context<'_>) -> Option<Poll<()>> {
        match self.to_session_manager.poll_reserve(cx) {
            Poll::Ready(Ok(())) => {
                let handle = self.pending_handle.take().unwrap();
                self.to_session_manager
                    .send_item(StromSessionMessage::Established { handle })
                    .unwrap();
                return None
            }
            Poll::Ready(Err(_)) => {
                // channel closed
            }
            Poll::Pending => return Some(Poll::Pending)
        }
        Some(Poll::Pending)
    }

    /// If a termination message is queued, this will try to send it to the
    /// manager
    fn poll_terminate_message(&mut self, cx: &mut Context<'_>) -> Option<Poll<Option<BytesMut>>> {
        let (mut tx, msg) = self.terminate_message.take()?;
        match tx.poll_reserve(cx) {
            Poll::Pending => {
                self.terminate_message = Some((tx, msg));
                return Some(Poll::Pending)
            }
            Poll::Ready(Ok(())) => {
                tracing::debug!("terminate_message");
                let _ = tx.send_item(msg);
            }
            Poll::Ready(Err(_)) => {
                // channel closed
            }
        }
        // terminate the task
        Some(Poll::Ready(None))
    }

    fn poll_commands(&mut self, cx: &mut Context<'_>) -> Option<Poll<Option<BytesMut>>> {
        self.commands_rx
            .poll_next_unpin(cx)
            .map(|inner| {
                inner.map_or_else(
                    || Poll::Ready(None),
                    |msg| match msg {
                        SessionCommand::Disconnect { .. } => self.emit_disconnect(cx),
                        SessionCommand::Message(msg) => {
                            let msg = StromProtocolMessage {
                                message_id: msg.message_id(),
                                message:    msg
                            };
                            let mut buf = BytesMut::new();

                            msg.encode(&mut buf);
                            Poll::Ready(Some(buf))
                        }
                    }
                )
            })
            .flatten()
            .some_if(|f| f.is_ready())
    }

    fn poll_incoming(&mut self, cx: &mut Context<'_>) -> Option<Poll<Option<BytesMut>>> {
        // processes incoming messages until there are none left or the stream closes
        while let Poll::Ready(msg) = self.conn.poll_next_unpin(cx).map(|data| {
            data.map(|bytes| {
                let msg = StromProtocolMessage::decode_message(&mut bytes.deref());

                let msg = msg
                    .map(|m| StromSessionMessage::ValidMessage {
                        peer_id: self.remote_peer_id,
                        message: m
                    })
                    .unwrap_or(StromSessionMessage::BadMessage { peer_id: self.remote_peer_id });
                self.outbound_buffer.push_back(msg);
            })
            .ok_or_else(|| self.emit_disconnect(cx))
        }) {
            if let Err(e) = msg {
                return Some(e)
            }
        }

        None
    }

    fn poll_verification(&mut self, cx: &mut Context<'_>) -> Poll<Option<BytesMut>> {
        if !self.verification_sidecar.has_sent {
            let msg = StromMessage::Status(
                self.verification_sidecar
                    .make_status_message(self.remote_peer_id)
            );
            // mark our status as sent.
            self.verification_sidecar.has_sent = true;

            let msg = StromProtocolMessage { message_id: msg.message_id(), message: msg };

            let mut buf = BytesMut::new();
            msg.encode(&mut buf);

            return Poll::Ready(Some(buf))
        }

        self.conn
            .poll_next_unpin(cx)
            .map(|msg| {
                // mark status as received. we do this here as the first message should be
                // status. if its not we want to disconnect which will be polled.
                self.verification_sidecar.has_received = true;

                msg.map(|bytes| {
                    let msg = StromProtocolMessage::decode_message(&mut bytes.deref());

                    msg.map_or(false, |msg| {
                        // first message has to be status
                        if let StromMessage::Status(status) = msg.message {
                            self.verify_incoming_status(status)
                        } else {
                            false
                        }
                    })
                })
                // if false, i.e verification failed. then we disconnect
                .filter(|f| *f)
                .map(|_| Poll::Pending)
                .unwrap_or_else(|| self.emit_disconnect(cx))
            })
            .flatten()
    }

    fn try_send_outbound(&mut self, cx: &mut Context<'_>) {
        while let Poll::Ready(Ok(_)) = self.to_session_manager.poll_reserve(cx) {
            if let Some(item) = self.outbound_buffer.pop_front() {
                self.to_session_manager.send_item(item).unwrap();
            } else {
                return
            }
        }
    }

    fn verify_incoming_status(&self, status: Status) -> bool {
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();

        let status_time = status.state.timestamp + STATUS_TIMESTAMP_TIMEOUT_MS;
        current_time <= status_time && status.verify() == Ok(self.remote_peer_id)
    }
}

impl Stream for StromSession {
    type Item = BytesMut;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.pending_handle.is_some() && self.poll_init_connection(cx).is_some() {
            return Poll::Pending
        }

        if !self.verification_sidecar.is_verified() {
            return self.poll_verification(cx)
        }

        // if the session is terminate we have to send the termination message before we
        // can close
        if let Some(terminate) = self.poll_terminate_message(cx) {
            return terminate
        }

        // progress manager commands
        if let Some(msg) = self.poll_commands(cx) {
            return msg
        }

        // processes messages from the wire
        if let Some(msg) = self.poll_incoming(cx) {
            return msg
        }

        self.try_send_outbound(cx);

        Poll::Pending
    }
}
