/// Where the transaction originates from.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum OrderOrigin {
    /// Order is coming from a local source.
    Local,
    /// Order has been received externally.
    ///
    /// This is usually considered an "untrusted" source, for example received
    /// from another in the network.
    External,
    /// Order originated locally and is intended to remain private.
    /// This type of Order should not be propagated to the network. It's
    /// meant for private usage within the local node, or other composable
    /// mev-angstroms.
    Private
}
