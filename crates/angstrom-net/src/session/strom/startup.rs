use std::{
    collections::HashSet,
    ops::Deref,
    time::{SystemTime, UNIX_EPOCH}
};

use alloy::{
    primitives::Address,
    rlp::{BytesMut, Encodable}
};
use angstrom_types::primitive::{AngstromMetaSigner, PeerId};
use futures::{
    StreamExt,
    task::{Context, Poll}
};
use reth_eth_wire::multiplex::ProtocolConnection;
use reth_metrics::common::mpsc::MeteredPollSender;
use tokio_stream::wrappers::ReceiverStream;

use super::{
    super::handle::SessionCommand, StromSession, StromSessionStates, VerificationSidecar,
    regular::RegularProcessing, shutdown::Shutdown
};
use crate::{
    StromMessage, StromSessionHandle, StromSessionMessage,
    types::{message::StromProtocolMessage, status::Status}
};

const STATUS_TIMESTAMP_TIMEOUT_MS: u128 = 1500;

pub struct StromStartup<S: AngstromMetaSigner> {
    verification_sidecar: VerificationSidecar<S>,
    handle:               Option<StromSessionHandle>,
    conn:                 ProtocolConnection,
    remote_peer_id:       PeerId,
    to_session_manager:   MeteredPollSender<StromSessionMessage>,
    commands_rx:          ReceiverStream<SessionCommand>,
    shutdown:             bool,
    valid_nodes:          HashSet<Address>
}

impl<S: AngstromMetaSigner> StromStartup<S> {
    pub fn new(
        verification_sidecar: VerificationSidecar<S>,
        handle: Option<StromSessionHandle>,
        conn: ProtocolConnection,
        remote_peer_id: PeerId,
        to_session_manager: MeteredPollSender<StromSessionMessage>,
        commands_rx: ReceiverStream<SessionCommand>,
        valid_nodes: HashSet<Address>
    ) -> Self {
        Self {
            verification_sidecar,
            handle,
            conn,
            remote_peer_id,
            to_session_manager,
            commands_rx,
            shutdown: false,
            valid_nodes
        }
    }

    fn manager_has_handle(&mut self, cx: &mut Context<'_>) -> bool {
        if self.handle.is_none() {
            return true;
        }

        match self.to_session_manager.poll_reserve(cx) {
            Poll::Ready(Ok(())) => {
                let handle = self.handle.take().unwrap();
                // Manager may be dropped during shutdown; ignore send errors.
                let _ = self
                    .to_session_manager
                    .send_item(StromSessionMessage::Established { handle });
                return true;
            }
            Poll::Ready(Err(_)) => {
                tracing::error!("channel closed");
                // channel closed
            }
            Poll::Pending => {}
        }
        false
    }

    fn handle_verification(&mut self, cx: &mut Context<'_>) -> Poll<Option<BytesMut>> {
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

            return Poll::Ready(Some(buf));
        }

        if let Poll::Ready(msg) = self.conn.poll_next_unpin(cx) {
            match msg {
                Some(data) => {
                    let msg = StromProtocolMessage::decode_message(&mut data.deref());
                    let valid_verification = msg.is_ok_and(|msg| {
                        // first message has to be status
                        if let StromMessage::Status(status) = msg.message {
                            tracing::debug!(?status, peer=?self.remote_peer_id, "decoded status message");

                            self.verify_incoming_status(status)
                        } else {
                            false
                        }
                    });
                    self.shutdown = !valid_verification;
                }
                None => {
                    self.shutdown = true;
                    cx.waker().wake_by_ref();
                }
            }
        } else {
            return Poll::Pending;
        }

        // we are ready to transition
        Poll::Ready(None)
    }

    fn verify_incoming_status(&self, status: Status) -> bool {
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();

        let status_time = status.state.timestamp + STATUS_TIMESTAMP_TIMEOUT_MS;
        let verification = status.verify();

        let Ok(verification) = verification else { return false };
        let peer_id = verification;
        let digest = alloy::primitives::keccak256(peer_id);
        let address = Address::from_slice(&digest[12..]);

        current_time <= status_time && self.valid_nodes.contains(&address)
    }
}

impl<S: AngstromMetaSigner> StromSession<S> for StromStartup<S> {
    fn poll_outbound_msg(&mut self, cx: &mut Context<'_>) -> Poll<Option<BytesMut>> {
        if !self.manager_has_handle(cx) {
            return Poll::Pending;
        }

        self.handle_verification(cx)
    }

    fn poll_next_state(self, cx: &mut Context<'_>) -> Option<StromSessionStates<S>> {
        // going to register a waker so that the new state will be registered
        cx.waker().wake_by_ref();

        if self.shutdown {
            Some(StromSessionStates::Shutdown(Shutdown::new(
                self.conn,
                self.remote_peer_id,
                self.to_session_manager,
                self.commands_rx
            )))
        } else {
            Some(StromSessionStates::Regular(RegularProcessing::new(
                self.conn,
                self.remote_peer_id,
                self.to_session_manager,
                self.commands_rx
            )))
        }
    }
}
