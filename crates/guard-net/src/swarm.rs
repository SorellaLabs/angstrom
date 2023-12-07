use std::{
    pin::Pin,
    task::{Context, Poll}
};

use futures::{Stream, StreamExt};
use reth_primitives::PeerId;

use crate::{
    peers::PeersManager,
    session::StromSessionManager,
    types::message::{StromMessage, StromProtocolMessage},
    PeerAction, PeerKind, SessionEvent
};

#[derive(Debug)]
#[must_use = "Swarm does nothing unless polled"]
pub(crate) struct Swarm {
    /// All sessions.
    sessions:      StromSessionManager,
    peers_manager: PeersManager
}

impl Swarm {
    /// Creates a new `Swarm`.
    pub fn new(sessions: StromSessionManager, peers_manager: PeersManager) -> Self {
        Swarm { sessions, peers_manager }
    }

    /// Access to the [`SessionManager`].
    pub(crate) fn sessions(&self) -> &StromSessionManager {
        &self.sessions
    }

    /// Mutable access to the [`SessionManager`].
    pub(crate) fn sessions_mut(&mut self) -> &mut StromSessionManager {
        &mut self.sessions
    }

    pub(crate) fn state(&self) -> &PeersManager {
        &self.peers_manager
    }

    pub(crate) fn state_mut(&mut self) -> &mut PeersManager {
        &mut self.peers_manager
    }

    pub(crate) fn remove_peer(&mut self, peer_id: PeerId, kind: PeerKind) {
        match kind {
            PeerKind::Basic => self.peers_manager.remove_peer(peer_id),
            PeerKind::MevGuard => self.peers_manager.remove_peer_from_trusted_set(peer_id),
            _ => todo!()
        }
    }

    fn on_session_event(&mut self, event: SessionEvent) -> Option<SwarmEvent> {
        match event {
            SessionEvent::BadMessage { peer_id } => {
                self.peers_manager
                    .change_weight(peer_id, crate::ReputationChangeKind::BadMessage);
                None
            }
            SessionEvent::ValidMessage { peer_id, message } => {
                Some(SwarmEvent::ValidMessage { peer_id, msg: message.message })
            }
            SessionEvent::Disconnected { peer_id } => Some(SwarmEvent::Disconnected { peer_id }),
            _ => None
        }
    }

    fn on_peer_action(&mut self, action: PeerAction) -> Option<SwarmEvent> {
        None
    }
}

impl Stream for Swarm {
    type Item = SwarmEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        while let Poll::Ready(Some(event)) = self.sessions.poll_next_unpin(cx) {
            if let Some(event) = self.on_session_event(event) {
                return Poll::Ready(Some(event))
            }
        }

        while let Some(action) = self.peers_manager.poll() {
            if let Some(res) = self.on_peer_action(action) {
                return Poll::Ready(Some(res))
            }
        }
        // Poll the session manager

        Poll::Pending
    }
}

pub enum SwarmEvent {
    ValidMessage { peer_id: PeerId, msg: StromMessage },
    Disconnected { peer_id: PeerId }
}
