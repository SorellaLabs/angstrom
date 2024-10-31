use std::{
    pin::Pin,
    task::{Context, Poll}
};

use angstrom_types::primitive::PeerId;
use futures::{Stream, StreamExt};

use crate::{
    session::StromSessionManager,
    state::{StateEvent, StromState},
    types::message::StromMessage,
    PeerKind, SessionEvent
};

#[derive(Debug)]
#[must_use = "Swarm does nothing unless polled"]
pub struct Swarm<DB> {
    /// All sessions.
    sessions: StromSessionManager,
    state:    StromState<DB>
}

impl<DB: Unpin> Swarm<DB> {
    /// Creates a new `Swarm`.
    pub fn new(sessions: StromSessionManager, state: StromState<DB>) -> Self {
        Swarm { sessions, state }
    }

    pub fn state(&self) -> &StromState<DB> {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut StromState<DB> {
        &mut self.state
    }

    pub fn sessions_mut(&mut self) -> &mut StromSessionManager {
        &mut self.sessions
    }

    pub(crate) fn remove_peer(&mut self, peer_id: PeerId, kind: PeerKind) {
        match kind {
            PeerKind::Basic => self.state.peers_mut().remove_peer(peer_id),
            PeerKind::MevGuard => self.state.peers_mut().remove_peer_from_trusted_set(peer_id),
            _ => todo!()
        }
    }

    fn on_session_event(&mut self, event: SessionEvent) -> Option<SwarmEvent> {
        match event {
            SessionEvent::BadMessage { peer_id } => {
                self.state
                    .peers_mut()
                    .change_weight(peer_id, crate::ReputationChangeKind::BadMessage);
                None
            }
            SessionEvent::ValidMessage { peer_id, message } => {
                Some(SwarmEvent::ValidMessage { peer_id, msg: message.message })
            }
            SessionEvent::Disconnected { peer_id } => Some(SwarmEvent::Disconnected { peer_id }),
            SessionEvent::SessionEstablished { peer_id, direction, timeout } => {
                Some(SwarmEvent::SessionEstablished { peer_id })
            }
            _ => None
        }
    }

    fn on_state_event(&mut self, action: StateEvent) -> Option<SwarmEvent> {
        tracing::warn!(?action, "no impl");
        None
    }
}

impl<DB: Unpin> Stream for Swarm<DB> {
    type Item = SwarmEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        while let Poll::Ready(Some(event)) = self.sessions.poll_next_unpin(cx) {
            if let Some(event) = self.on_session_event(event) {
                return Poll::Ready(Some(event))
            }
        }

        while let Some(action) = self.state.poll(cx) {
            if let Some(res) = self.on_state_event(action) {
                return Poll::Ready(Some(res))
            }
        }

        Poll::Pending
    }
}

pub enum SwarmEvent {
    SessionEstablished { peer_id: PeerId },
    ValidMessage { peer_id: PeerId, msg: StromMessage },
    Disconnected { peer_id: PeerId }
}
