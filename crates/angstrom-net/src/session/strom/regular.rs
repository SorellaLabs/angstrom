use std::{collections::VecDeque, ops::Deref};

use alloy::rlp::{BytesMut, Encodable};
use angstrom_types::primitive::{AngstromMetaSigner, PeerId};
use futures::{
    StreamExt,
    task::{Context, Poll}
};
use reth_eth_wire::multiplex::ProtocolConnection;
use reth_metrics::common::mpsc::MeteredPollSender;
use tokio_stream::wrappers::ReceiverStream;

use super::{super::handle::SessionCommand, StromSession, StromSessionStates, shutdown::Shutdown};
use crate::{StromSessionMessage, types::message::StromProtocolMessage};

pub struct RegularProcessing {
    conn:               ProtocolConnection,
    remote_peer_id:     PeerId,
    to_session_manager: MeteredPollSender<StromSessionMessage>,
    commands_rx:        ReceiverStream<SessionCommand>,
    manager_buffer:     VecDeque<StromSessionMessage>
}

impl RegularProcessing {
    pub fn new(
        conn: ProtocolConnection,
        remote_peer_id: PeerId,
        to_session_manager: MeteredPollSender<StromSessionMessage>,
        commands_rx: ReceiverStream<SessionCommand>
    ) -> Self {
        Self {
            conn,
            remote_peer_id,
            to_session_manager,
            commands_rx,
            manager_buffer: VecDeque::new()
        }
    }

    fn send_to_manager(&mut self, cx: &mut Context<'_>) {
        while let Poll::Ready(Ok(_)) = self.to_session_manager.poll_reserve(cx) {
            let Some(next) = self.manager_buffer.pop_front() else {
                return;
            };
            // If the manager is dropped (e.g., during shutdown), ignore send errors.
            let _ = self.to_session_manager.send_item(next);
        }
    }
}

impl<S: AngstromMetaSigner> StromSession<S> for RegularProcessing {
    fn poll_outbound_msg(&mut self, cx: &mut Context<'_>) -> Poll<Option<BytesMut>> {
        // handle possible commands
        if let Poll::Ready(possible_command) = self.commands_rx.poll_next_unpin(cx) {
            match possible_command {
                Some(command) => match command {
                    SessionCommand::Disconnect { .. } => return Poll::Ready(None),
                    SessionCommand::Message(msg) => {
                        let msg =
                            StromProtocolMessage { message_id: msg.message_id(), message: msg };
                        let mut buf = BytesMut::new();

                        msg.encode(&mut buf);
                        return Poll::Ready(Some(buf));
                    }
                },
                None => return Poll::Ready(None)
            }
        }

        // now that we have handled possible outbound messages, lets handle the inbound
        // messages
        while let Poll::Ready(bytes) = self.conn.poll_next_unpin(cx) {
            match bytes {
                Some(bytes) => {
                    let msg = StromProtocolMessage::decode_message(&mut bytes.deref());

                    let msg = msg
                        .map(|m| StromSessionMessage::ValidMessage {
                            peer_id: self.remote_peer_id,
                            message: m
                        })
                        .unwrap_or_else(|_| StromSessionMessage::BadMessage {
                            peer_id: self.remote_peer_id
                        });
                    self.manager_buffer.push_back(msg);
                }
                // only once all messages that are pending are sent do we return
                None if self.manager_buffer.is_empty() => return Poll::Ready(None),
                None => {}
            }
        }

        self.send_to_manager(cx);

        Poll::Pending
    }

    fn poll_next_state(self, cx: &mut Context<'_>) -> Option<StromSessionStates<S>> {
        cx.waker().wake_by_ref();

        Some(StromSessionStates::Shutdown(Shutdown::new(
            self.conn,
            self.remote_peer_id,
            self.to_session_manager,
            self.commands_rx
        )))
    }
}
