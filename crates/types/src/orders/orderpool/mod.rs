use std::fmt::Debug;

use alloy_primitives::{Address, B256, U256};
use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::primitive::PoolId;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Encode, Decode)]
pub struct OrderId {
    #[bincode(with_serde)]
    pub address:  Address,
    /// Pool id
    pub pool_id:  PoolId,
    /// Hash of the order. Needed to check for inclusion
    #[bincode(with_serde)]
    pub hash:     B256,
    /// Nonce of the order
    #[bincode(with_serde)]
    pub nonce:    U256,
    /// when the order expires
    #[bincode(with_serde)]
    pub deadline: U256,
    /// Order Location
    pub location: OrderLocation
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
pub struct OrderPriorityData {
    pub price:  u128,
    pub volume: u128,
    pub gas:    u128
}

impl PartialOrd for OrderPriorityData {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderPriorityData {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.price
            .cmp(&other.price)
            .then_with(|| self.volume.cmp(&other.volume))
            .then_with(|| self.gas.cmp(&other.gas))
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Default, Encode, Decode)]
pub enum OrderLocation {
    #[default]
    Limit,
    Searcher
}

#[derive(Debug, Clone, Error)]
pub enum ValidationError {
    #[error("{0}")]
    StateValidationError(#[from] StateValidationError),
    #[error("bad signer")]
    BadSigner
}

#[derive(Debug, Error, Clone)]
pub enum StateValidationError {
    #[error("order: {0:?} nonce was invalid: {1}")]
    InvalidNonce(B256, U256),
    #[error("order: {0:?} did not have enough of {1:?}")]
    NotEnoughApproval(B256, Address),
    #[error("order: {0:?} did not have enough of {1:?}")]
    NotEnoughBalance(B256, Address)
}
