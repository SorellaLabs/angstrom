use std::collections::{hash_map::Entry, HashMap, VecDeque};

use reth_eth_wire::DisconnectReason;
use reth_net_banlist::BanList;
use reth_network_peers::PeerId;
use tracing::trace;

pub use super::reputation::ReputationChangeWeights;
use super::reputation::{is_banned_reputation, ReputationChangeKind};

/// Maintains the state of _all_ the peers known to the network.
///
/// This is supposed to be owned by the network itself, but can be reached via
/// the [`PeersHandle`]. From this type, connections to peers are established or
/// disconnected, see [`PeerAction`].
///
/// The [`PeersManager`] will be notified on peer related changes
#[derive(Debug)]
pub struct PeersManager {
    /// All peers known to the network
    peers:              HashMap<PeerId, Peer>,
    /// Buffered actions until the manager is polled.
    queued_actions:     VecDeque<PeerAction>,
    /// How to weigh reputation changes
    reputation_weights: ReputationChangeWeights,
    /// Tracks unwanted ips/peer ids.
    ban_list:           BanList
}

impl Default for PeersManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PeersManager {
    pub fn new() -> Self {
        Self {
            peers:              HashMap::new(),
            queued_actions:     VecDeque::new(),
            reputation_weights: ReputationChangeWeights::default(),
            ban_list:           BanList::default()
        }
    }

    /// Removes the tracked node from the set.
    pub fn remove_peer(&mut self, peer_id: PeerId) {
        let Entry::Occupied(entry) = self.peers.entry(peer_id) else { return };
        if entry.get().is_trusted() {
            return
        }

        trace!(target: "angstrom::net::peers",  ?peer_id, "remove discovered node");
        self.queued_actions
            .push_back(PeerAction::PeerRemoved(peer_id));
    }

    pub fn change_weight(&mut self, peer_id: PeerId, weight: ReputationChangeKind) {
        if let Some(outcome) = self
            .peers
            .get_mut(&peer_id)
            .map(|peer| peer.apply_reputation(self.reputation_weights.change(weight).into()))
        {
            match outcome {
                ReputationChangeOutcome::Ban => self.ban_list.ban_peer(peer_id),
                ReputationChangeOutcome::DisconnectAndBan => {
                    self.ban_list.ban_peer(peer_id);
                    self.queued_actions
                        .push_back(PeerAction::DisconnectBannedIncoming { peer_id })
                }
                ReputationChangeOutcome::Unban => self
                    .queued_actions
                    .push_back(PeerAction::UnBanPeer { peer_id }),
                ReputationChangeOutcome::None => {}
            }
        }
    }

    /// Removes the tracked node from the trusted set.
    pub fn remove_peer_from_trusted_set(&mut self, peer_id: PeerId) {
        let Entry::Occupied(mut entry) = self.peers.entry(peer_id) else { return };
        if !entry.get().is_trusted() {
            return
        }

        let peer = entry.get_mut();

        peer.kind = PeerKind::Basic;
    }

    pub fn poll(&mut self) -> Option<PeerAction> {
        self.queued_actions.pop_front()
    }
}

// /// Commands the [`PeersManager`] listens for.
// #[derive(Debug)]
// pub(crate) enum PeerCommand {
//     /// Command for manually add
//     Add(PeerId),
//     /// Remove a peer from the set
//     ///
//     /// If currently connected this will disconnect the session
//     Remove(PeerId),
//     /// Apply a reputation change to the given peer.
//     ReputationChange(PeerId, ReputationChangeKind),
//     /// Get information about a peer
//     GetPeer(PeerId, oneshot::Sender<Option<Peer>>),
//     /// Get node information on all peers
//     GetPeers(oneshot::Sender<Vec<NodeRecord>>)
// }

/// Represents the kind of peer
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub enum PeerKind {
    /// Basic peer kind.
    #[default]
    Basic,
    /// Mev-Guard
    MevGuard,
    /// Trusted peer kind.
    Trusted,
    /// Trusted mev guard
    TrustedMevGuard
}

/// Tracks info about a single peer.
#[derive(Debug, Clone)]
pub struct Peer {
    /// Reputation of the peer.
    reputation: i32,
    /// The kind of peer
    kind:       PeerKind,
    /// if peer is connected
    connected:  bool
}

/// Outcomes when a reputation change is applied to a peer
enum ReputationChangeOutcome {
    /// Nothing to do.
    None,
    /// Ban the peer.
    Ban,
    /// Ban and disconnect
    DisconnectAndBan,
    /// Unban the peer
    Unban
}

// === impl Peer ===

impl Peer {
    // fn new(kind: PeerKind, trusted: bool, connected: bool) -> Self {
    //     Peer { reputation: DEFAULT_REPUTATION, kind, trusted, connected }
    // }
    //
    // /// Resets the reputation of the peer to the default value. This always
    // /// returns [`ReputationChangeOutcome::None`].
    // fn reset_reputation(&mut self) -> ReputationChangeOutcome {
    //     self.reputation = DEFAULT_REPUTATION;
    //
    //     ReputationChangeOutcome::None
    // }

    /// Applies a reputation change to the peer and returns what action should
    /// be taken.
    fn apply_reputation(&mut self, reputation: i32) -> ReputationChangeOutcome {
        let previous = self.reputation;
        // we add reputation since negative reputation change decrease total reputation
        self.reputation = previous.saturating_add(reputation);

        trace!(target: "angstrom::net::peers", reputation=%self.reputation, banned=%self.is_banned(), "applied reputation change");

        if self.connected && self.is_banned() {
            self.connected = false;
            return ReputationChangeOutcome::DisconnectAndBan
        }

        if self.is_banned() && !is_banned_reputation(previous) {
            return ReputationChangeOutcome::Ban
        }

        if !self.is_banned() && is_banned_reputation(previous) {
            return ReputationChangeOutcome::Unban
        }

        ReputationChangeOutcome::None
    }

    /// Returns true if the peer's reputation is below the banned threshold.
    #[inline]
    fn is_banned(&self) -> bool {
        is_banned_reputation(self.reputation)
    }

    // /// Unbans the peer by resetting its reputation
    // #[inline]
    // fn unban(&mut self) {
    //     self.reputation = DEFAULT_REPUTATION
    // }

    /// Returns whether this peer is trusted
    #[inline]
    fn is_trusted(&self) -> bool {
        matches!(self.kind, PeerKind::Trusted)
    }
}

/// Actions the peer manager can trigger.
#[derive(Debug)]
pub enum PeerAction {
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
    },
    /// Emit peerAdded event
    PeerAdded(PeerId),
    /// Emit peerRemoved event
    PeerRemoved(PeerId)
}
