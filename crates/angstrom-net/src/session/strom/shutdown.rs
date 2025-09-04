use alloy::rlp::BytesMut;
use angstrom_types::primitive::{AngstromMetaSigner, PeerId};
use futures::{
    StreamExt,
    task::{Context, Poll}
};
use reth_eth_wire::multiplex::ProtocolConnection;
use reth_metrics::common::mpsc::MeteredPollSender;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::PollSender;

use super::{super::handle::SessionCommand, StromSession, StromSessionStates};
use crate::StromSessionMessage;

pub struct Shutdown {
    // we hold this to ensure that we don't drop before we have handled everything on our end
    _conn:              ProtocolConnection,
    remote_peer_id:     PeerId,
    to_session_manager: Option<PollSender<StromSessionMessage>>,
    commands_rx:        ReceiverStream<SessionCommand>
}

impl Shutdown {
    pub fn new(
        conn: ProtocolConnection,
        remote_peer_id: PeerId,
        to_session_manager: MeteredPollSender<StromSessionMessage>,
        commands_rx: ReceiverStream<SessionCommand>
    ) -> Self {
        Self {
            _conn: conn,
            remote_peer_id,
            to_session_manager: Some(to_session_manager.inner().clone()),
            commands_rx
        }
    }
}

impl<S: AngstromMetaSigner> StromSession<S> for Shutdown {
    fn poll_outbound_msg(&mut self, cx: &mut Context<'_>) -> Poll<Option<BytesMut>> {
        if let Some(mut inner) = self.to_session_manager.take() {
            // Only proceed if we successfully reserved capacity.
            if matches!(inner.poll_reserve(cx), Poll::Ready(Ok(()))) {
                // Session manager may already be dropped during shutdown; ignore send errors.
                let _ = inner
                    .send_item(StromSessionMessage::Disconnected { peer_id: self.remote_peer_id });
                cx.waker().wake_by_ref();
            } else {
                self.to_session_manager = Some(inner);
            }
        } else {
            // once this returns Poll::Ready(None) we know that the shutdown has been
            // registered and we can drop this
            while let Poll::Ready(cmd) = self.commands_rx.poll_next_unpin(cx) {
                if cmd.is_none() {
                    tracing::info!(?self.remote_peer_id, "properly shutdown peer");
                    return Poll::Ready(None);
                }
            }
        }

        Poll::Pending
    }

    fn poll_next_state(self, _: &mut Context<'_>) -> Option<StromSessionStates<S>> {
        None
    }
}
