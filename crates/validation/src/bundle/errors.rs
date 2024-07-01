use alloy_sol_types::Error;
use angstrom_types::rpc::SignedLimitOrder;
use reth_errors::RethError;
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
        matches!(self, SimResult::ExecutionResult(_))
    }
}

#[derive(Debug, Error)]
pub enum SimError {
    #[error("Unable to read Revm-Reth StateProvider Database")]
    RevmDatabaseError(#[from] RethError),
    #[error("Unable to Create Runtime For ThreadPool")]
    RuntimeCreationError(#[from] std::io::Error),
    #[error("Unable to Start libmdbx transaction")]
    LibmdbxTransactionError(#[from] reth_db::mdbx::Error),
    #[error("Error Decoding EIP712 Transaction With Serde: {0:#?}")]
    SerdeEip712DecodingError(#[from] serde_json::error::Error),
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

impl From<Signature> for SimError {
    fn from(value: Signature) -> Self {
        SimError::RecoveringSignerError(value)
    }
}
