use std::{collections::HashSet, sync::Arc, task::Context};

use alloy::{primitives::Address, sol};
use angstrom_types::primitive::PeerId;
use parking_lot::RwLock;
use reth_network::DisconnectReason;

use crate::PeersManager;

sol! {
    function validators() public view returns(address[]);
}

#[derive(Debug)]
pub struct StromState<DB> {
    peers_manager: PeersManager,

    db:           DB,
    active_peers: HashSet<PeerId>,
    validators:   Arc<RwLock<HashSet<Address>>>
}

impl<DB> StromState<DB> {
    pub fn new(db: DB, validators: Arc<RwLock<HashSet<Address>>>) -> Self {
        Self { peers_manager: PeersManager::new(), db, validators, active_peers: HashSet::new() }
    }

    pub fn peers_mut(&mut self) -> &mut PeersManager {
        &mut self.peers_manager
    }

    pub fn validators(&self) -> Arc<RwLock<HashSet<Address>>> {
        self.validators.clone()
    }

    pub fn is_active_peer(&self, peer_id: PeerId) -> bool {
        self.active_peers.contains(&peer_id)
    }

    pub fn poll(&mut self, cx: &mut Context<'_>) -> Option<StateEvent> {
        self.peers_manager.poll().map(|action| match action {
            crate::PeerAction::Disconnect { peer_id, reason } => {
                StateEvent::Disconnect { peer_id, reason }
            }
            crate::PeerAction::BanPeer { peer_id } => StateEvent::BanPeer { peer_id },
            crate::PeerAction::DisconnectBannedIncoming { peer_id } => {
                StateEvent::DisconnectBannedIncoming { peer_id }
            }
            crate::PeerAction::UnBanPeer { peer_id } => StateEvent::UnBanPeer { peer_id },
            _ => unreachable!()
        })
    }
}

#[derive(Debug)]
pub enum StateEvent {
    /// Disconnect an existing connection.
    Disconnect {
        /// The peer ID of the established connection.
        peer_id: PeerId,
        /// An optional reason for the disconnect.
        reason:  Option<DisconnectReason>
    },
    /// Disconnect an existing incoming connection, because the peers reputation
    /// is below the banned threshold or is on the [`BanList`]
    DisconnectBannedIncoming {
        /// The peer ID of the established connection.
        peer_id: PeerId
    },
    /// Ban the peer temporarily
    BanPeer {
        /// The peer ID.
        peer_id: PeerId
    },
    /// Unban the peer temporarily
    UnBanPeer {
        /// The peer ID.
        peer_id: PeerId
    }
}
