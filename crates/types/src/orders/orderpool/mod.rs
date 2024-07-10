use std::fmt::Debug;

use alloy_primitives::{Address, B256, U256};
use thiserror::Error;

use crate::primitive::PoolId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderId {
    pub address:  Address,
    /// Pool id
    pub pool_id:  PoolId,
    /// Hash of the order. Needed to check for inclusion
    pub hash:     B256,
    /// Nonce of the order
    pub nonce:    U256,
    /// when the order expires
    pub deadline: U256,
    /// Order Location
    pub location: OrderLocation
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderLocation {
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
