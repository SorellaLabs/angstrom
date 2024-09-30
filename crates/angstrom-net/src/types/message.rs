#![allow(missing_docs)]
use std::{fmt::Debug, sync::Arc};

use alloy::rlp::{Decodable, Encodable};
use angstrom_types::{
    consensus::{Commit, PreProposal, Proposal},
    sol_bindings::grouped_orders::AllOrders
};
use reth_eth_wire::{protocol::Protocol, Capability};
use reth_network_p2p::error::RequestError;
use reth_primitives::bytes::{Buf, BufMut};
use serde::{Deserialize, Serialize};

use crate::errors::StromStreamError;
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    fn decode(buf: &mut &[u8]) -> Result<Self, alloy::rlp::Error> {
        let id = buf.first().ok_or(alloy::rlp::Error::InputTooShort)?;
        let id = match id {
            0 => StromMessageID::Status,
            1 => StromMessageID::PrePropose,
            2 => StromMessageID::Propose,
            3 => StromMessageID::Commit,
            4 => StromMessageID::PropagatePooledOrders,
            _ => return Err(alloy::rlp::Error::Custom("Invalid message ID"))
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
        let data: Vec<u8> = Decodable::decode(buf)?;
        let message: StromMessage = bincode::deserialize(&data).unwrap();

        Ok(StromProtocolMessage { message_id, message })
    }
}

impl Encodable for StromProtocolMessage {
    fn encode(&self, out: &mut dyn BufMut) {
        Encodable::encode(&self.message_id, out);
        let buf = bincode::serialize(&self.message).unwrap();
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StromMessage {
    /// init
    Status(Status),
    /// TODO: do we need a status ack?

    /// Consensus
    PrePropose(PreProposal),
    Propose(Proposal),
    Commit(Box<Commit>),

    /// Propagation messages that broadcast new orders to all peers
    PropagatePooledOrders(Vec<AllOrders>)
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StromBroadcastMessage {
    // Consensus Broadcast
    PrePropose(Arc<PreProposal>),
    Propose(Arc<Proposal>),
    Commit(Arc<Commit>),
    // Order Broadcast
    PropagatePooledOrders(Arc<Vec<AllOrders>>)
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
