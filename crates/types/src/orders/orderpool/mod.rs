use std::fmt::Debug;

use alloy::primitives::{Address, B256, U256};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    primitive::PoolId,
    sol_bindings::{ext::RespendAvoidanceMethod, RawPoolOrder}
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OrderStatus {
    Filled,
    Pending,
    Blocked
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OrderId {
    /// user address
    pub address:         Address,
    /// Pool id
    pub pool_id:         PoolId,
    /// Hash of the order. Needed to check for inclusion
    pub hash:            B256,
    /// reuse avoidance
    pub reuse_avoidance: RespendAvoidanceMethod,
    /// when the order expires
    pub deadline:        Option<U256>,
    pub flash_block:     Option<u64>,
    /// Order Location
    pub location:        OrderLocation
}

impl OrderId {
    pub fn from_all_orders<T: RawPoolOrder>(order: &T, pool_id: PoolId) -> Self {
        OrderId {
            reuse_avoidance: order.respend_avoidance_strategy(),
            flash_block: order.flash_block(),
            address: order.from(),
            pool_id,
            hash: order.order_hash(),
            deadline: order.deadline(),
            location: order.order_location()
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderPriorityData {
    pub price:     U256,
    pub volume:    u128,
    /// gas used in the pairs token0
    pub gas:       U256,
    /// gas units used
    pub gas_units: u64
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
            .then_with(|| self.gas_units.cmp(&other.gas_units))
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Default, Serialize, Deserialize)]
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
