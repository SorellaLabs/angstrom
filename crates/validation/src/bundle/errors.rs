use ethers_core::types::transaction::eip712::{Eip712Error, TypedData};
use guard_types::rpc::SignedLimitOrder;
use reth_primitives::{
    revm_primitives::{Account, Address, HashMap, TxEnv},
    Signature
};
use thiserror::Error;
use tokio::sync::mpsc::error::SendError;

use crate::{
    bundle::{BundleOrTransactionResult, BundleSimRequest},
    validator::ValidationRequest
};

#[derive(Debug)]
pub enum SimResult {
    /// error running a sim on the evm
    SimError(SimError),
    /// execution result of the sim
    ExecutionResult(BundleOrTransactionResult),
    /// the evm/caches were updated succesfully
    SuccessfulRevmBlockUpdate
}

impl SimResult {
    pub fn is_success(&self) -> bool {
        match self {
            SimResult::ExecutionResult(_) => true,
            _ => false
        }
    }
}

#[derive(Debug, Error)]
pub enum SimError {
    #[error("Unable to read Revm-Reth StateProvider Database")]
    RevmDatabaseError(reth_interfaces::RethError),
    #[error("Unable to Create Runtime For ThreadPool")]
    RuntimeCreationError(std::io::Error),
    #[error("Unable to Start libmdbx transaction")]
    LibmdbxTransactionError(reth_db::mdbx::Error),
    #[error("Error Encoding EIP712 Transaction: {0:#?}")]
    Eip712EncodingError(Eip712Error),
    #[error("Error Decoding EIP712 Transaction With Serde: {0:#?}")]
    SerdeEip712DecodingError(serde_json::error::Error),
    #[error("Error decoding signature: {0:#?}")]
    DecodingSignatureError(SignedLimitOrder),
    #[error("No Transactions Decoded from EIP712 Tx: {0:#?}")]
    NoTransactionsInEip712(TxEnv),
    #[error("EVM Simulation Error: {0:#?}")]
    RevmEVMTransactionError(TxEnv),
    #[error("Revm Cache Error: {0:#?}")]
    RevmCacheError((TxEnv, HashMap<Address, Account>)),
    #[error("Call instead of create transaction: {0:#?}")]
    CallInsteadOfCreateError(TxEnv),
    #[error("Error Decoding EIP712 Transaction: {0:#?}")]
    Eip712DecodingError(TypedData),
    #[error("No verifying contract on EIP712 transaction: {0:#?}")]
    NoVerifyingContract(TxEnv),
    #[error("Error decoding signature: {0:#?}")]
    RecoveringSignerError(Signature),
    #[error("Simulation reverted: {0:#?}")]
    SimRevert(TxEnv),
    #[error("Simulation halted: {0:#?}")]
    SimHalt(TxEnv),
    #[error("Invalid amount in after hook: expected: {0:#?} got: {1:#?}")]
    InvalidAmountIn(u128, u128),
    #[error("hook failed")]
    HookFailed,
    #[error("V4 reverted")]
    V4Failed,
    #[error("sending to validation failed")]
    SendToValidationError(#[from] SendError<ValidationRequest>)
}

impl From<std::io::Error> for SimError {
    fn from(value: std::io::Error) -> Self {
        SimError::RuntimeCreationError(value)
    }
}

impl From<reth_db::mdbx::Error> for SimError {
    fn from(value: reth_db::mdbx::Error) -> Self {
        SimError::LibmdbxTransactionError(value)
    }
}

impl From<reth_interfaces::RethError> for SimError {
    fn from(value: reth_interfaces::RethError) -> Self {
        SimError::RevmDatabaseError(value)
    }
}

impl From<Eip712Error> for SimError {
    fn from(value: Eip712Error) -> Self {
        SimError::Eip712EncodingError(value)
    }
}

impl From<serde_json::error::Error> for SimError {
    fn from(value: serde_json::error::Error) -> Self {
        SimError::SerdeEip712DecodingError(value)
    }
}

impl From<TypedData> for SimError {
    fn from(value: TypedData) -> Self {
        SimError::Eip712DecodingError(value)
    }
}

impl From<Signature> for SimError {
    fn from(value: Signature) -> Self {
        SimError::RecoveringSignerError(value)
    }
}
