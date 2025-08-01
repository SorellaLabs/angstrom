use futures::Stream;
use angstrom_types::{
    network::{PoolNetworkMessage, ReputationChangeKind},
    primitive::PeerId
};

use crate::StromNetworkEvent;

/// Trait for network handles used by pool manager
pub trait NetworkHandle: Send + Sync {
    type Events<'a>: Stream<Item = StromNetworkEvent> + Send + Unpin
    where
        Self: 'a;

    fn send_message(&mut self, peer_id: PeerId, message: PoolNetworkMessage);
    fn peer_reputation_change(&mut self, peer_id: PeerId, change: ReputationChangeKind);
    fn subscribe_network_events(&self) -> Self::Events<'_>;
}