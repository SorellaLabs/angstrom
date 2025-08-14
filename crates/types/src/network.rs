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
    Reset
}

impl ReputationChangeKind {
    /// Returns true if the reputation change is a reset.
    pub fn is_reset(&self) -> bool {
        matches!(self, Self::Reset)
    }
}

/// Pool-specific message types for network communication
#[derive(Clone)]
pub enum PoolNetworkMessage {
    PropagatePooledOrders(Vec<crate::sol_bindings::grouped_orders::AllOrders>),
    OrderCancellation(crate::orders::CancelOrderRequest)
}
