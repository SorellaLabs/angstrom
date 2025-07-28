use reth_eth_wire::DisconnectReason;

use crate::{
    orders::CancelOrderRequest,
    primitive::PeerId,
    sol_bindings::grouped_orders::AllOrders,
};

/// (Non-exhaustive) Events emitted by the network that are of interest for
/// subscribers.
///
/// This includes any event types that may be relevant to tasks, for metrics,
/// keep track of peers etc.
#[derive(Debug, Clone)]
pub enum StromNetworkEvent {
    /// Closed the peer session.
    SessionClosed {
        /// The identifier of the peer to which a session was closed.
        peer_id: PeerId,
        /// Why the disconnect was triggered
        reason: Option<DisconnectReason>,
    },
    /// Established a new session with the given peer.
    SessionEstablished {
        /// The identifier of the peer to which a session was established.
        peer_id: PeerId,
    },
    /// Event emitted when a new peer is added
    PeerAdded(PeerId),
    /// Event emitted when a new peer is removed
    PeerRemoved(PeerId),
}

/// All events related to orders emitted by the network.
#[derive(Debug, Clone, PartialEq)]
pub enum NetworkOrderEvent {
    IncomingOrders { peer_id: PeerId, orders: Vec<AllOrders> },
    CancelOrder { peer_id: PeerId, request: CancelOrderRequest },
}

/// Various kinds of reputation changes.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ReputationChangeKind {
    /// Received an unknown message from the peer
    BadMessage,
    /// Peer sent a bad order, i.e. an order who's signature isn't recoverable
    BadOrder,
    /// Peer sent an invalid composable order, invalidity know at state n - 1
    BadComposableOrder,
    /// Peer sent a bad bundle, i.e. a bundle that is invalid
    BadBundle,
    /// a order that failed validation
    InvalidOrder,
    /// Reset the reputation to the default value.
    Reset,
}

impl ReputationChangeKind {
    /// Returns true if the reputation change is a reset.
    pub fn is_reset(&self) -> bool {
        matches!(self, Self::Reset)
    }
}

/// Pool-specific message types for network communication
#[derive(Clone)]
pub enum PoolStromMessage {
    PropagatePooledOrders(Vec<crate::sol_bindings::grouped_orders::AllOrders>),
    OrderCancellation(crate::orders::CancelOrderRequest),
}

/// Trait for network handles used by pool manager
pub trait NetworkHandle: Send + Sync {
    fn send_message(&mut self, peer_id: PeerId, message: PoolStromMessage);
    fn peer_reputation_change(&mut self, peer_id: PeerId, change: ReputationChangeKind);
    fn subscribe_network_events(&self) -> tokio_stream::wrappers::UnboundedReceiverStream<StromNetworkEvent>;
}