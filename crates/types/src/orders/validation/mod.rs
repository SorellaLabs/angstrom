use std::fmt::Debug;

use derive_more::{AsRef, Deref};
use thiserror::Error;
pub mod limit;
pub mod searcher;
use alloy_primitives::{Address, B256, U256};
pub use limit::*;
use reth_primitives::TxHash;
pub use searcher::*;

use crate::{orders::PoolOrder, primitive::PoolId};

#[derive(Debug, AsRef, Deref, Clone)]
pub struct ValidatedOrder<O: PoolOrder, Data: Clone + Debug> {
    #[deref]
    #[as_ref]
    pub order:    O,
    pub pool_id:  usize,
    pub is_bid:   bool,
    pub location: OrderLocation,
    pub data:     Data
}

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

impl<O: PoolOrder, Data: Clone + Debug> From<ValidatedOrder<O, Data>> for OrderId {
    fn from(order: ValidatedOrder<O, Data>) -> Self {
        Self {
            address:  order.order.from(),
            pool_id:  order.pool_id,
            hash:     order.order.hash(),
            nonce:    order.order.nonce(),
            deadline: order.order.deadline(),
            location: order.location
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderLocation {
    Composable,
    LimitParked,
    LimitPending,
    VanillaSearcher,
    ComposableSearcher
}

impl OrderLocation {
    pub fn is_limit_pending(&self) -> bool {
        matches!(self, OrderLocation::LimitPending)
    }
}

pub enum ValidationResults<L, CL, S, CS>
where
    L: PoolOrder,
    CL: PoolOrder,
    S: PoolOrder,
    CS: PoolOrder
{
    Limit(OrderValidationOutcome<L>),
    ComposableLimit(OrderValidationOutcome<CL>),
    Searcher(OrderValidationOutcome<S>),
    ComposableSearcher(OrderValidationOutcome<CS>)
}

/// A valid order in the pool.
#[derive(Debug)]
pub enum OrderValidationOutcome<O: PoolOrder> {
    /// The transaction is considered _currently_ valid and can be inserted into
    /// the pool.
    Valid {
        /// The validated order
        order:     ValidatedOrder<O, O::ValidationData>,
        /// Whether to propagate the order to the network.
        propagate: bool
    },
    /// The transaction is considered invalid indefinitely: It violates
    /// constraints that prevent this transaction from ever becoming valid.
    Invalid(O, ValidationError),
    /// An error occurred while trying to validate the transaction
    Error(TxHash, Box<dyn std::error::Error + Send + Sync>)
}

impl<O: PoolOrder> OrderValidationOutcome<O> {
    pub fn is_valid(&self) -> bool {
        matches!(self, OrderValidationOutcome::Valid { .. })
    }
}

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("{0}")]
    StateValidationError(#[from] StateValidationError)
}

#[derive(Debug, Error)]
pub enum StateValidationError {
    #[error("order: {0:?} nonce was invalid: {1}")]
    InvalidNonce(B256, U256),
    #[error("order: {0:?} did not have enough of {1:?}")]
    NotEnoughApproval(B256, Address),
    #[error("order: {0:?} did not have enough of {1:?}")]
    NotEnoughBalance(B256, Address)
}
