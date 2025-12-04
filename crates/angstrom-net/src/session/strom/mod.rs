pub mod regular;
pub mod shutdown;
pub mod startup;

use std::{collections::HashSet, fmt::Debug, pin::Pin};

use alloy::{primitives::Address, rlp::BytesMut};
use angstrom_types::primitive::{AngstromMetaSigner, AngstromSigner, PeerId};
use futures::{
    Stream,
    task::{Context, Poll}
};
use regular::RegularProcessing;
use reth_eth_wire::multiplex::ProtocolConnection;
use reth_metrics::common::mpsc::MeteredPollSender;
use shutdown::Shutdown;
use startup::StromStartup;
use tokio_stream::wrappers::ReceiverStream;

use super::handle::SessionCommand;
use crate::{
    StatusBuilder, StromSessionHandle, StromSessionMessage,
    types::status::{Status, StatusState}
};

pub enum StromSessionStates<S: AngstromMetaSigner> {
    Startup(StromStartup<S>),
    Regular(RegularProcessing),
    Shutdown(Shutdown),
    Dummy(DummyState)
}

impl<S: AngstromMetaSigner> Default for StromSessionStates<S> {
    fn default() -> Self {
        Self::Dummy(DummyState)
    }
}

impl<S: AngstromMetaSigner> StromSession<S> for StromSessionStates<S> {
    fn poll_outbound_msg(&mut self, cx: &mut Context<'_>) -> Poll<Option<BytesMut>> {
        match self {
            Self::Startup(s) => s.poll_outbound_msg(cx),
            Self::Regular(s) => <_ as StromSession<S>>::poll_outbound_msg(s, cx),
            Self::Shutdown(s) => <_ as StromSession<S>>::poll_outbound_msg(s, cx),
            Self::Dummy(s) => <_ as StromSession<S>>::poll_outbound_msg(s, cx)
        }
    }

    fn poll_next_state(self, cx: &mut Context<'_>) -> Option<StromSessionStates<S>> {
        match self {
            Self::Startup(s) => s.poll_next_state(cx),
            Self::Regular(s) => s.poll_next_state(cx),
            Self::Shutdown(s) => s.poll_next_state(cx),
            Self::Dummy(s) => s.poll_next_state(cx)
        }
    }
}

/// this trait handles the transition and different functionality of
/// a strom session at the different points in time
pub trait StromSession<S: AngstromMetaSigner>: Send + 'static {
    /// Messages encoded that are meant for the peer
    fn poll_outbound_msg(&mut self, cx: &mut Context<'_>) -> Poll<Option<BytesMut>>;
    /// will transition to next state.
    fn poll_next_state(self, cx: &mut Context<'_>) -> Option<StromSessionStates<S>>;
}

#[derive(Default)]
pub struct DummyState;
impl<S: AngstromMetaSigner> StromSession<S> for DummyState {
    fn poll_outbound_msg(&mut self, _: &mut Context<'_>) -> Poll<Option<BytesMut>> {
        Poll::Ready(None)
    }

    fn poll_next_state(self, _: &mut Context<'_>) -> Option<StromSessionStates<S>> {
        None
    }
}

/// holds the state we need to verify the new peer
#[derive(Clone)]
pub struct VerificationSidecar<S: AngstromMetaSigner> {
    pub secret_key:   AngstromSigner<S>,
    pub status:       StatusState,
    pub has_sent:     bool,
    pub has_received: bool
}

impl<S: AngstromMetaSigner> VerificationSidecar<S> {
    pub fn make_status_message(&mut self, peer: PeerId) -> Status {
        if self.has_sent {
            panic!("can only send the status message once");
        }

        StatusBuilder::from(self.status.with_peer(peer)).build(&self.secret_key)
    }

    pub fn is_verified(&self) -> bool {
        self.has_sent && self.has_received
    }
}

impl<S: AngstromMetaSigner> Debug for VerificationSidecar<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("status: {:?}", self.status))
    }
}

pub struct StromSessionHandler<S: AngstromMetaSigner> {
    inner_state: StromSessionStates<S>
}

impl<S: AngstromMetaSigner> StromSessionHandler<S> {
    pub fn new(
        conn: ProtocolConnection,
        peer_id: PeerId,
        commands_rx: ReceiverStream<SessionCommand>,
        to_session_manager: MeteredPollSender<StromSessionMessage>,
        verification_sidecar: VerificationSidecar<S>,
        handle: StromSessionHandle,
        valid_nodes: HashSet<Address>
    ) -> Self {
        let inner_state = StromSessionStates::Startup(StromStartup::new(
            verification_sidecar,
            Some(handle),
            conn,
            peer_id,
            to_session_manager,
            commands_rx,
            valid_nodes
        ));
        Self { inner_state }
    }
}

impl<S: AngstromMetaSigner> Stream for StromSessionHandler<S> {
    type Item = BytesMut;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.as_mut();
        if let Poll::Ready(next) = this.inner_state.poll_outbound_msg(cx) {
            match next {
                data @ Some(_) => return Poll::Ready(data),
                // transition to next state
                None => {
                    let prev = std::mem::take(&mut this.inner_state);
                    let Some(new_state) = prev.poll_next_state(cx) else {
                        return Poll::Ready(None);
                    };

                    this.inner_state = new_state;
                }
            }
        }
        Poll::Pending
    }
}
