use std::{ops::Deref, pin::Pin};

use alloy_rlp::Encodable;
use futures::{
    task::{Context, Poll},
    Stream, StreamExt
};
use reth_eth_wire::multiplex::ProtocolConnection;
use reth_metrics::common::mpsc::MeteredPollSender;
use reth_primitives::{BytesMut, PeerId};
use tokio::time::Duration;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::PollSender;

use super::handle::SessionCommand;
use crate::{types::message::StromProtocolMessage, StromSessionMessage};
#[allow(dead_code)]
pub struct StromSession {
    /// Keeps track of request ids.
    pub(crate) next_id:            u64,
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
    pub(crate) terminate_message: Option<(PollSender<StromSessionMessage>, StromSessionMessage)>
}

impl StromSession {
    pub fn new(
        conn: ProtocolConnection,
        peer_id: PeerId,
        commands_rx: ReceiverStream<SessionCommand>,
        to_session_manager: MeteredPollSender<StromSessionMessage>,
        protocol_breach_request_timeout: Duration
    ) -> Self {
        Self {
            next_id: 0,
            conn,
            remote_peer_id: peer_id,
            commands_rx,
            to_session_manager,
            protocol_breach_request_timeout,
            terminate_message: None
        }
    }

    /// Report back that this session has been closed.
    fn emit_disconnect(&mut self, cx: &mut Context<'_>) -> Poll<Option<BytesMut>> {
        let msg = StromSessionMessage::Disconnected { peer_id: self.remote_peer_id };

        self.terminate_message = Some((self.to_session_manager.inner().clone(), msg));
        self.poll_terminate_message(cx).expect("message is set")
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
                let _ = tx.send_item(msg);
            }
            Poll::Ready(Err(_)) => {
                // channel closed
            }
        }
        // terminate the task
        Some(Poll::Ready(None))
    }
}

//TODO: Implement poll functionality with: on_command, on_timeout, on_message
// from wire..

impl Stream for StromSession {
    type Item = BytesMut;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        // if the session is terminate we have to send the termination message before we
        // can close
        if let Some(terminate) = this.poll_terminate_message(cx) {
            return terminate
        }

        'main: loop {
            let mut progress = false;

            // we prioritize incoming commands sent from the session manager
            loop {
                match this.commands_rx.poll_next_unpin(cx) {
                    Poll::Pending => break,
                    Poll::Ready(None) => {
                        // this is only possible when the manager was dropped, in which case we also
                        // terminate this session
                        return Poll::Ready(None)
                    }
                    Poll::Ready(Some(command)) => {
                        return match command {
                            //TODO: maybe we could find a way to disconnect by sending the
                            // underlying disconnect reason to the wire
                            // so the peer receives it
                            SessionCommand::Disconnect { reason: _reason } => {
                                this.emit_disconnect(cx)
                            }

                            SessionCommand::Message(msg) => {
                                let msg = StromProtocolMessage {
                                    message_type: msg.message_id(),
                                    message:      msg
                                };
                                let mut bytes = BytesMut::with_capacity(msg.length());
                                msg.encode(&mut bytes);
                                Poll::Ready(Some(bytes))
                            }
                        }
                    }
                }
            }

            loop {
                match this.conn.poll_next_unpin(cx) {
                    Poll::Pending => break,
                    Poll::Ready(None) => {
                        // the connection was closed, we have to terminate the session
                        return this.emit_disconnect(cx)
                    }
                    Poll::Ready(Some(bytes)) => {
                        let msg = StromProtocolMessage::decode_message(&mut bytes.deref());
                        match msg {
                            Ok(msg) => {
                                let _ = this.to_session_manager.send_item(
                                    StromSessionMessage::ValidMessage {
                                        peer_id: this.remote_peer_id,
                                        message: msg
                                    }
                                );
                                progress = true;
                            }
                            Err(_e) => {
                                let _ = this.to_session_manager.send_item(
                                    StromSessionMessage::BadMessage {
                                        peer_id: this.remote_peer_id
                                    }
                                );
                                break
                            }
                        }
                    }
                }
            }
            if !progress {
                break 'main
            }
        }
        Poll::Pending
    }
}
