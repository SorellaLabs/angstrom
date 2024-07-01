#![allow(missing_docs)]
use std::{fmt::Debug, sync::Arc};

use alloy_rlp::{Decodable, Encodable};
use angstrom_types::{
    consensus::{Commit, PreProposal, Proposal},
    orders::PooledOrder
};
use reth_eth_wire::{capability::Capability, protocol::Protocol};
use reth_network_p2p::error::RequestError;
use reth_primitives::bytes::{Buf, BufMut};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::errors::StromStreamError;
/// Result alias for result of a request.
pub type RequestResult<T> = Result<T, RequestError>;
use crate::Status;

/// [`MAX_MESSAGE_SIZE`] is the maximum cap on the size of a protocol message.
// https://github.com/ethereum/go-ethereum/blob/30602163d5d8321fbc68afdcbbaf2362b2641bde/eth/protocols/eth/protocol.go#L50
pub const MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024;

const STROM_CAPABILITY: Capability = Capability::new_static("strom", 1);
const STROM_PROTOCOL: Protocol = Protocol::new(STROM_CAPABILITY, 7);

/// An `eth` protocol message, containing a message ID and payload.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct StromProtocolMessage {
    pub message_type: StromMessageID,
    pub message:      StromMessage
}

impl StromProtocolMessage {
    /// Create a new ProtocolMessage from a message type and message rlp bytes.
    pub fn decode_message(buf: &mut &[u8]) -> Result<Self, StromStreamError> {
        let message_type = StromMessageID::decode(buf)?;

        let message = match message_type {
            StromMessageID::Status => StromMessage::Status(Status::decode(buf)?),
            StromMessageID::PrePropose => StromMessage::PrePropose(PreProposal::decode(buf)?),
            StromMessageID::Propose => StromMessage::Propose(Proposal::decode(buf)?),
            StromMessageID::Commit => StromMessage::Commit(Box::new(Commit::decode(buf)?)),
            StromMessageID::PropagatePooledOrders => {
                StromMessage::PropagatePooledOrders(Vec::<PooledOrder>::decode(buf)?)
            }
        };
        Ok(StromProtocolMessage { message_type, message })
    }

    /// Returns the protocol for the `Strom` protocol.
    pub const fn protocol() -> Protocol {
        STROM_PROTOCOL
    }
}

/// Encodes the protocol message into bytes.
/// The message type is encoded as a single byte and prepended to the message.
impl Encodable for StromProtocolMessage {
    fn encode(&self, out: &mut dyn BufMut) {
        self.message_type.encode(out);
        self.message.encode(out);
    }

    fn length(&self) -> usize {
        self.message_type.length() + self.message.length()
    }
}

/// Represents messages that can be sent to multiple peers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtocolBroadcastMessage {
    pub message_type: StromMessageID,
    pub message:      StromBroadcastMessage
}

/// Encodes the protocol message into bytes.
/// The message type is encoded as a single byte and prepended to the message.
impl Encodable for ProtocolBroadcastMessage {
    fn encode(&self, out: &mut dyn BufMut) {
        self.message_type.encode(out);
        self.message.encode(out);
    }

    fn length(&self) -> usize {
        self.message_type.length() + self.message.length()
    }
}

impl From<StromBroadcastMessage> for ProtocolBroadcastMessage {
    fn from(message: StromBroadcastMessage) -> Self {
        ProtocolBroadcastMessage { message_type: message.message_id(), message }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum StromMessage {
    /// init
    Status(Status),
    /// TODO: do we need a status ack?

    /// Consensus
    PrePropose(PreProposal),
    Propose(Proposal),
    Commit(Box<Commit>),

    /// Propagation messages that broadcast new orders to all peers
    PropagatePooledOrders(Vec<PooledOrder>)
}

macro_rules! encodable_enum {
    ($enum_name:ident, $($var:ident),+) => {
        impl Encodable for $enum_name {
            fn encode(&self, out: &mut dyn BufMut) {
                match self {
                    $( $enum_name::$var(t) => t.encode(out), )*
                }
            }

            fn length(&self) -> usize {
                match self {
                    $( $enum_name::$var(t) => t.length(), )*
                }
            }
        }
    };
}

encodable_enum!(StromMessage, Status, PrePropose, Propose, Commit, PropagatePooledOrders);

impl StromMessage {
    /// Returns the message's ID.
    pub fn message_id(&self) -> StromMessageID {
        match self {
            StromMessage::Status(_) => StromMessageID::Status,
            StromMessage::PrePropose(_) => StromMessageID::PrePropose,
            StromMessage::Propose(_) => StromMessageID::Propose,
            StromMessage::Commit(_) => StromMessageID::Commit,
            StromMessage::PropagatePooledOrders(_) => StromMessageID::PropagatePooledOrders
        }
    }
}

/// Represents broadcast messages of [`StromMessage`] with the same object that
/// can be sent to multiple peers.
///
/// Messages that contain a list of hashes depend on the peer the message is
/// sent to. A peer should never receive a hash of an object (block,
/// transaction) it has already seen.
///
/// Note: This is only useful for outgoing messages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StromBroadcastMessage {
    // Consensus Broadcast
    PrePropose(Arc<PreProposal>),
    Propose(Arc<Proposal>),
    Commit(Arc<Commit>),
    // Order Broadcast
    PropagatePooledOrders(Arc<Vec<PooledOrder>>)
}

// === impl StromBroadcastMessage ===
impl StromBroadcastMessage {
    /// Returns the message's ID.
    pub fn message_id(&self) -> StromMessageID {
        match self {
            StromBroadcastMessage::PrePropose(_) => StromMessageID::PrePropose,
            StromBroadcastMessage::Propose(_) => StromMessageID::Propose,
            StromBroadcastMessage::Commit(_) => StromMessageID::Commit,
            StromBroadcastMessage::PropagatePooledOrders(_) => StromMessageID::PropagatePooledOrders
        }
    }
}

encodable_enum!(StromBroadcastMessage, PrePropose, Propose, Commit, PropagatePooledOrders);

/// Represents message IDs for eth protocol messages.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum StromMessageID {
    Status     = 0,
    /// Consensus
    PrePropose = 1,
    Propose    = 2,
    Commit     = 3,
    /// Propagation messages that broadcast new orders to all peers
    PropagatePooledOrders = 4
}

impl Encodable for StromMessageID {
    fn encode(&self, out: &mut dyn BufMut) {
        out.put_u8(*self as u8);
    }

    fn length(&self) -> usize {
        1
    }
}

impl Decodable for StromMessageID {
    fn decode(buf: &mut &[u8]) -> Result<Self, alloy_rlp::Error> {
        let id = buf.first().ok_or(alloy_rlp::Error::InputTooShort)?;
        let id = match id {
            0 => StromMessageID::Status,
            1 => StromMessageID::PrePropose,
            2 => StromMessageID::Propose,
            3 => StromMessageID::Commit,
            4 => StromMessageID::PropagatePooledOrders,
            _ => return Err(alloy_rlp::Error::Custom("Invalid message ID"))
        };
        buf.advance(1);
        Ok(id)
    }
}

impl TryFrom<usize> for StromMessageID {
    type Error = &'static str;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(StromMessageID::Status),
            1 => Ok(StromMessageID::PrePropose),
            2 => Ok(StromMessageID::Propose),
            3 => Ok(StromMessageID::Commit),
            4 => Ok(StromMessageID::PropagatePooledOrders),
            _ => Err("Invalid message ID")
        }
    }
}
