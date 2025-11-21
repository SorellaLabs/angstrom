use alloy_primitives::{Address, B256};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::primitive::PoolId;

#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize, PartialEq, Eq)]
pub enum OrderValidationError {
    #[error(transparent)]
    StateError(#[from] UserAccountVerificationError),
    #[error("min qty on partial < max gas amount")]
    InvalidPartialOrder,
    #[error("invalid signature")]
    InvalidSignature,
    #[error("invalid pool")]
    InvalidPool,
    #[error("not enough gas t0")]
    NotEnoughGas,
    #[error("duplicate order")]
    DuplicateOrder,
    #[error("not valid at block")]
    InvalidOrderAtBlock,
    #[error("no amount specified")]
    NoAmountSpecified,
    #[error("max gas for this order is greater than min amount")]
    MaxGasGreaterThanMinAmount,
    #[error("no gas was specified for this order")]
    NoGasSpecified,
    #[error("no price was specified for this order")]
    NoPriceSpecified,
    #[error("The delta for the parital order is to small. Check docs to see set mins")]
    MinDeltaToSmall,
    #[error("the price was outside of the supported pool's range")]
    PriceOutOfPoolBounds,
    #[error("order was cancelled")]
    CancelledOrder,
    #[error("{err}")]
    Unknown { err: String }
}

#[derive(Debug, Error, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum UserAccountVerificationError {
    #[error("the input or output generates a invalid tob swap")]
    InvalidToBSwap,
    #[error("tried to verify for block {} where current is {}", requested, current)]
    BlockMissMatch { requested: u64, current: u64, pool_info: UserOrderPoolInfo },
    #[error("order hash has been cancelled {order_hash:?}")]
    OrderIsCancelled { order_hash: B256 },
    #[error("Nonce exists for a current order hash: {order_hash:?}")]
    DuplicateNonce { order_hash: B256 },
    #[error(
        "block for flash order is not for next block. next_block: {next_block}, requested_block: \
         {requested_block}."
    )]
    BadBlock { next_block: u64, requested_block: u64 },
    #[error("currently hooks are not supported. this field should be empty bytes")]
    NonEmptyHook,
    #[error("could not fetch, error - {err}")]
    CouldNotFetch { err: String },
    #[error("{order_hash:?} insufficient approval amounts. token {token_in:?} needs {amount} more")]
    InsufficientApproval { order_hash: B256, token_in: Address, amount: u128 },

    #[error("{order_hash:?} insufficient balance amounts. token {token_in:?} needs {amount} more")]
    InsufficientBalance { order_hash: B256, token_in: Address, amount: u128 },
    #[error(
        "{order_hash:?} insufficient balance and approval amounts. token {token_in:?} needs \
         {amount_balance} more balance and {amount_approval} more approvals"
    )]
    InsufficientBoth {
        order_hash:      B256,
        token_in:        Address,
        amount_balance:  u128,
        amount_approval: u128
    },

    #[error("not enough gas nneeded: {needed_gas} max_set {set_gas}")]
    NotEnoughGas { needed_gas: u128, set_gas: u128 },
    #[error("{err}")]
    Unknown { err: String }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserOrderPoolInfo {
    // token in for pool
    pub token:   Address,
    pub is_bid:  bool,
    pub pool_id: PoolId
}
