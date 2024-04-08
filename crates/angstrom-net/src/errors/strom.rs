//! Error handling for Strom protocol stream
use reth_primitives::GotExpected;

/// Errors when sending/receiving messages
#[derive(thiserror::Error, Debug)]
pub enum StromStreamError {
    #[error("Strom handshake failed")]
    /// Failed Ethereum handshake.
    StromHandshakeError(StromHandshakeError),
    #[error("message size ({0}) exceeds max length (10MB)")]
    /// Received a message whose size exceeds the standard limit.
    MessageTooBig(usize),
    #[error("message id is invalid")]
    /// Flags an unrecognized message ID for a given protocol version.
    InvalidMessageError
}

/// Error  that can occur during the `eth` sub-protocol handshake.
#[derive(thiserror::Error, Debug)]
pub enum StromHandshakeError {
    /// Status message received or sent outside of the handshake process.
    #[error("status message can only be recv/sent in handshake")]
    StatusNotInHandshake,
    /// Receiving a non-status message during the handshake phase.
    #[error("received non-status message when trying to handshake")]
    NonStatusMessageInHandshake,
    #[error("no response received when sending out handshake")]
    /// No response received during the handshake process.
    NoResponse,
    #[error("mismatched protocol version in status message: {0}")]
    /// Mismatched protocol versions in status messages.
    MismatchedProtocolVersion(GotExpected<u8>),
    #[error("Not a valid Stale-Guard node: {0}")]
    /// The guard node does not have sufficient stake to be a validator
    NotAValidGuardNode(usize),
    #[error("Invalid signature on stake verification message")]
    /// The signature on the stake verification message is invalid / does not
    /// match the staking address' public key
    InvalidStakeVerificationSignature
}

impl From<alloy_rlp::Error> for StromStreamError {
    fn from(_err: alloy_rlp::Error) -> Self {
        StromStreamError::InvalidMessageError
    }
}
