/// The type that tracks the reputation score.
pub type Reputation = i32;

/// The default reputation of a peer
pub(crate) const DEFAULT_REPUTATION: Reputation = 0;

/// The minimal unit we're measuring reputation
const REPUTATION_UNIT: i32 = -1024;

/// The reputation value below which new connection from/to peers are rejected.
pub(crate) const BANNED_REPUTATION: i32 = 50 * REPUTATION_UNIT;

/// The reputation change when a peer sends a bad message.
pub(crate) const BAD_MESSAGE_REPUTATION_CHANGE: Reputation = 5 * REPUTATION_UNIT;

/// The reputation change when a peer sends a bad order.
pub(crate) const BAD_ORDER_REPUTATION_CHANGE: Reputation = 10 * REPUTATION_UNIT;

/// The reputation change when a peer sends an invalid composable order.
pub(crate) const BAD_COMPOSABLE_ORDER_REPUTATION_CHANGE: Reputation = 15 * REPUTATION_UNIT;

/// The reputation change when a peer sends a bad bundle.
pub(crate) const BAD_BUNDLE_REPUTATION_CHANGE: Reputation = 20 * REPUTATION_UNIT;

/// The reputation change when a peer sends a invalid order
pub(crate) const INVALID_ORDER_REPUTATION_CHANGE: Reputation = 17 * REPUTATION_UNIT;

/// Various kinds of stale guard specific reputation changes.
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

/// Returns `true` if the given reputation is below the [`BANNED_REPUTATION`]
/// threshold
#[inline]
pub(crate) fn is_banned_reputation(reputation: i32) -> bool {
    reputation < BANNED_REPUTATION
}

/// How the [`ReputationChangeKind`] are weighted.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ReputationChangeWeights {
    /// Weight for [`ReputationChangeKind::BadMessage`]
    pub bad_message:          Reputation,
    /// Weight for [`ReputationChangeKind::BadOrder`]
    pub bad_order:            Reputation,
    /// Weight for [`ReputationChangeKind::BadComposableOrder`]
    pub bad_composable_order: Reputation,
    /// Weight for [`ReputationChangeKind::BadBundle`]
    pub bad_bundle:           Reputation,
    /// Weight for [`ReputationChangeKind::InvalidOrder`]
    pub invalid_order:        Reputation
}

impl Default for ReputationChangeWeights {
    fn default() -> Self {
        Self {
            bad_message:          BAD_MESSAGE_REPUTATION_CHANGE,
            bad_order:            BAD_ORDER_REPUTATION_CHANGE,
            bad_composable_order: BAD_COMPOSABLE_ORDER_REPUTATION_CHANGE,
            bad_bundle:           BAD_BUNDLE_REPUTATION_CHANGE,
            invalid_order:        INVALID_ORDER_REPUTATION_CHANGE
        }
    }
}

impl ReputationChangeWeights {
    /// Returns the quantifiable [`ReputationChange`] for the given
    /// [`ReputationChangeKind`] using the configured weights
    pub(crate) fn change(&self, kind: ReputationChangeKind) -> ReputationChange {
        match kind {
            ReputationChangeKind::BadMessage => self.bad_message.into(),
            ReputationChangeKind::BadOrder => self.bad_order.into(),
            ReputationChangeKind::BadComposableOrder => self.bad_composable_order.into(),
            ReputationChangeKind::BadBundle => self.bad_bundle.into(),
            ReputationChangeKind::InvalidOrder => self.invalid_order.into(),
            ReputationChangeKind::Reset => DEFAULT_REPUTATION.into()
        }
    }
}

/// Represents a change in a peer's reputation.
#[derive(Debug, Copy, Clone, Default)]
pub(crate) struct ReputationChange(Reputation);

impl From<ReputationChange> for Reputation {
    fn from(value: ReputationChange) -> Self {
        value.0
    }
}

impl From<Reputation> for ReputationChange {
    fn from(value: Reputation) -> Self {
        ReputationChange(value)
    }
}
