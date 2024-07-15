#![allow(missing_docs)]
use std::{borrow::BorrowMut, fmt::Debug, sync::Arc};

use alloy_rlp::{Decodable, Encodable, RlpDecodable, RlpEncodable};
use angstrom_types::{
    consensus::{Commit, PreProposal, Proposal},
    orders::PooledOrder,
    sol_bindings::grouped_orders::AllOrders
};
use bincode::{config::standard, decode_from_slice, encode_into_slice, Decode, Encode};
use reth_eth_wire::{capability::Capability, protocol::Protocol};
use reth_network_p2p::error::RequestError;
use reth_primitives::bytes::{Buf, BufMut};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::{errors::StromStreamError, StromSessionMessage};
/// Result alias for result of a request.
pub type RequestResult<T> = Result<T, RequestError>;
use crate::Status;

/// [`MAX_MESSAGE_SIZE`] is the maximum cap on the size of a protocol message.
// https://github.com/ethereum/go-ethereum/blob/30602163d5d8321fbc68afdcbbaf2362b2641bde/eth/protocols/eth/protocol.go#L50
pub const MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024;

const STROM_CAPABILITY: Capability = Capability::new_static("strom", 1);
const STROM_PROTOCOL: Protocol = Protocol::new(STROM_CAPABILITY, 5);
/// Represents message IDs for eth protocol messages.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Encode, Decode)]
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

/// An `eth` protocol message, containing a message ID and payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StromProtocolMessage {
    pub message_id: StromMessageID,
    pub message:    StromMessage
}

impl StromProtocolMessage {
    pub fn decode_message(buf: &mut &[u8]) -> Result<Self, StromStreamError> {
        let message_id: StromMessageID = Decodable::decode(buf)?;
        let message: StromMessage = decode_from_slice(buf, standard()).map(|f| f.0).unwrap();

        Ok(StromProtocolMessage { message_id, message })
    }
}

impl Encodable for StromProtocolMessage {
    fn encode(&self, out: &mut dyn BufMut) {
        Encodable::encode(&self.message_id, out);
        let mut buf = Vec::new();
        let bytes = encode_into_slice(self.message.clone(), &mut buf, standard()).unwrap();
        Encodable::encode(&buf, out);
    }
}

impl StromProtocolMessage {
    /// Returns the protocol for the `Strom` protocol.
    pub const fn protocol() -> Protocol {
        STROM_PROTOCOL
    }
}

/// Represents messages that can be sent to multiple peers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtocolBroadcastMessage {
    pub message_id: StromMessageID,
    pub message:    StromBroadcastMessage
}

impl From<StromBroadcastMessage> for ProtocolBroadcastMessage {
    fn from(message: StromBroadcastMessage) -> Self {
        ProtocolBroadcastMessage { message_id: message.message_id(), message }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub enum StromMessage {
    /// init
    Status(Status),
    /// TODO: do we need a status ack?

    /// Consensus
    PrePropose(#[bincode(with_serde)] PreProposal),
    Propose(#[bincode(with_serde)] Proposal),
    Commit(#[bincode(with_serde)] Box<Commit>),

    /// Propagation messages that broadcast new orders to all peers
    PropagatePooledOrders(#[bincode(with_serde)] Vec<AllOrders>)
}
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
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub enum StromBroadcastMessage {
    // Consensus Broadcast
    PrePropose(#[bincode(with_serde)] Arc<PreProposal>),
    Propose(#[bincode(with_serde)] Arc<Proposal>),
    Commit(#[bincode(with_serde)] Arc<Commit>),
    // Order Broadcast
    PropagatePooledOrders(#[bincode(with_serde)] Arc<Vec<AllOrders>>)
}

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
