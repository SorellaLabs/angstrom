use std::fmt::Debug;

use client::RevmClient;
use errors::{SimError, SimResult};
use ethers_core::types::{transaction::eip2718::TypedTransaction, I256, U256};
use guard_types::{
    primitive::{Angstrom::Bundle, ExternalStateSim},
    rpc::{CallerInfo, SignedComposableLimitOrder, SignedLimitOrder}
};
use tokio::sync::{mpsc::unbounded_channel, oneshot::Sender};

use crate::revm::Revm;

// pub mod anvil;
pub mod client;
pub mod errors;
pub mod executor;
pub mod lru_db;
pub mod revm;
pub mod slot_keeper;
pub mod state;

pub fn spawn_revm_sim(db: lru_db::RevmLRU) -> Result<RevmClient, SimError> {
    let (tx, rx) = unbounded_channel();
    std::thread::spawn(move || {
        let revm_client = Revm::new(rx, db).unwrap();
        let handle = revm_client.get_threadpool_handle();

        handle.block_on(revm_client);
    });

    Ok(RevmClient::new(tx))
}

#[derive(Debug)]
pub enum BundleOrTransactionResult {
    /// We just return the bundle as we don't care about gas usage but rather
    /// it finishes execution
    Bundle(Bundle),
    /// We just return the bundle as we don't care about gas usage but rather
    /// it finishes execution
    MevBundle(Bundle),
    HookSimResult {
        tx:            SignedComposableLimitOrder,
        pre_hook_gas:  U256,
        post_hook_gas: U256
    },
    UniswapV4Results {
        delta: I256,
        gas:   U256
    }
}

// the simulator is a handle that we use to simulate transactions.
#[async_trait::async_trait]
pub trait Simulator: Send + Sync + Clone + Unpin {
    /// executes the swap on the underlying v4 pool in order to see what the
    /// limit price for everyone will be
    async fn simulate_v4_tx(&self, tx: TypedTransaction) -> Result<SimResult, SimError>;
    /// executes the pre and post hook for the transactions to get the slots
    /// they touched and the cumulative gas that the pre and post hook use.
    /// this also checks to make sure we have enough value to execute on
    /// angstrom given there specifed amount in. we then for post hook give
    /// them there limit price they specifed and simulate that.
    async fn simulate_external_state<T>(
        &self,
        hook_data: T,
        caller_info: CallerInfo
    ) -> Result<SimResult, SimError>
    where
        T: TryInto<ExternalStateSim> + Send,
        <T as TryInto<ExternalStateSim>>::Error: Debug;

    /// simulates the full bundle in order to make sure it is valid and passes
    async fn simulate_vanilla_bundle(
        &self,
        caller_info: CallerInfo,
        bundle: Bundle
    ) -> Result<SimResult, SimError>;

    /// simulates the full bundle in order to make sure it is valid and passes
    async fn simulate_composable_bundle(
        &self,
        caller_info: CallerInfo,
        bundle: Bundle
    ) -> Result<SimResult, SimError>;
}

/// enum of transaction type
pub enum SimEvent {
    Hook(ExternalStateSim, CallerInfo, Sender<SimResult>),
    UniswapV4(TypedTransaction, Sender<SimResult>),
    Bundle(Bundle, CallerInfo, Sender<SimResult>),
    MevBundle(Bundle, CallerInfo, Sender<SimResult>),
    NewBlock(Sender<SimResult>)
}
