#![allow(missing_docs)]
use std::{fmt::Debug, sync::Arc};

use alloy_rlp::{Decodable, Encodable};
use angstrom_types::{
    consensus::{Commit, PreProposal, Proposal},
    orders::PooledOrder
};
use bincode::{Decode, Encode};
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
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub struct StromProtocolMessage {
    pub message: StromMessage
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
    pub message: StromBroadcastMessage
}

impl From<StromBroadcastMessage> for ProtocolBroadcastMessage {
    fn from(message: StromBroadcastMessage) -> Self {
        ProtocolBroadcastMessage { message }
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
    PropagatePooledOrders(#[bincode(with_serde)] Vec<PooledOrder>)
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
    PropagatePooledOrders(#[bincode(with_serde)] Arc<Vec<PooledOrder>>)
}
