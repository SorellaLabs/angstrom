use alloy::primitives::{Address, B256};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::primitive::PoolId;

#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize, PartialEq, Eq)]
pub enum OrderValidationError {
    #[error(transparent)]
    StateError(#[from] UserAccountVerificationError),
    #[error("the input or output generates a invalid tob swap")]
    InvalidToBSwap,
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
    #[error("the price was outside of the supported pool's range")]
    PriceOutOfPoolBounds,
    #[error("order was cancelled")]
    CancelledOrder,
    #[error("unknown")]
    Unknown(String)
}

#[derive(Debug, Error, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum UserAccountVerificationError {
    #[error("tried to verify for block {} where current is {}", requested, current)]
    BlockMissMatch { requested: u64, current: u64, pool_info: UserOrderPoolInfo },
    #[error("order hash has been cancelled {0:?}")]
    OrderIsCancelled(B256),
    #[error("Nonce exists for a current order hash: {0:?}")]
    DuplicateNonce(B256),
    #[error("block for flash order is not for next block. next_block: {0}, requested_block: {1}.")]
    BadBlock(u64, u64),
    #[error("currently hooks are not supported. this field should be empty bytes")]
    NonEmptyHook,
    #[error("could not fetch, error - {0}")]
    CouldNotFetch(String),
    #[error("{0} insufficient approval amounts. token {1} needs {2} more")]
    InsufficientApproval(B256, Address, u128),
    #[error("{0} insufficient balance amounts. token {1} needs {2} more")]
    InsufficientBalance(B256, Address, u128),
    #[error(
        "{0} insufficient balance and approval amounts. token {1} needs {2} more balance and {3} \
         more approvals"
    )]
    InsufficientBoth(B256, Address, u128, u128),
    #[error("{0}")]
    Unknown(String)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserOrderPoolInfo {
    // token in for pool
    pub token:   Address,
    pub is_bid:  bool,
    pub pool_id: PoolId
}
