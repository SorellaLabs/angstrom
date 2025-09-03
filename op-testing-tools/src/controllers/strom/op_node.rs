use std::{pin::Pin, sync::Arc};

use alloy::signers::local::PrivateKeySigner;
use angstrom_cli::handles::RollupHandles;
use angstrom_types::{primitive::AngstromSigner, testnet::InitialTestnetState};
use futures::Future;
use parking_lot::Mutex;
use reth_tasks::TaskExecutor;

use crate::{
    agents::AgentConfig,
    controllers::strom::OpNodeInternals,
    providers::{AnvilProvider, AnvilStateProvider, WalletProvider},
    types::{GlobalTestingConfig, WithWalletProvider, config::TestingNodeConfig},
    validation::TestOrderValidator
};

/// Minimal OP testnet node: no custom networking or consensus.
pub struct OpTestnetNode<P, G> {
    testnet_node_id: u64,
    state_provider:  AnvilProvider<P>,
    _init_state:     InitialTestnetState,
    config:          TestingNodeConfig<G>,
    validation:      Arc<Mutex<TestOrderValidator<AnvilStateProvider<WalletProvider>>>>,
    /// Internal shutdown signal used to gracefully stop background tasks
    shutdown_tx:     tokio::sync::watch::Sender<bool>
}

impl<P, G> OpTestnetNode<P, G>
where
    P: WithWalletProvider,
    G: GlobalTestingConfig
{
    pub async fn new<F>(
        node_config: TestingNodeConfig<G>,
        state_provider: AnvilProvider<P>,
        inital_angstrom_state: InitialTestnetState,
        agents: Vec<F>,
        executor: TaskExecutor
    ) -> eyre::Result<Self>
    where
        F: for<'a> Fn(
                &'a InitialTestnetState,
                AgentConfig
            ) -> Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + 'a>>
            + Clone
    {
        // Create shutdown signal for graceful termination of spawned tasks
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        let handles = RollupHandles::new();

        let (internals, validator) = OpNodeInternals::new(
            node_config.clone(),
            state_provider,
            handles,
            inital_angstrom_state.clone(),
            agents,
            executor.clone(),
            shutdown_rx.clone()
        )
        .await?;

        let state_provider = internals.state_provider;

        Ok(Self {
            testnet_node_id: node_config.node_id,
            state_provider,
            _init_state: inital_angstrom_state,
            config: node_config,
            validation: Arc::new(Mutex::new(validator)),
            shutdown_tx
        })
    }

    pub fn testnet_node_id(&self) -> u64 {
        self.testnet_node_id
    }

    pub fn testnet_node_config(&self) -> TestingNodeConfig<G> {
        self.config.clone()
    }

    pub fn state_provider(&self) -> &AnvilProvider<P> {
        &self.state_provider
    }

    pub fn get_sk(&self) -> AngstromSigner<PrivateKeySigner> {
        self.config.angstrom_signer()
    }

    pub async fn testnet_future(self) {
        // Keep the node alive (no networking/consensus to drive here)
        // TODO(mempirate): poll the validation future to completion I guess?
        futures::future::pending::<()>().await;
    }

    /// Signal all internal tasks to shut down gracefully.
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }

    /// Cloneable sender to trigger shutdown from external code.
    pub fn shutdown_sender(&self) -> tokio::sync::watch::Sender<bool> {
        self.shutdown_tx.clone()
    }

    pub fn strom_validation<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&TestOrderValidator<AnvilStateProvider<WalletProvider>>) -> R
    {
        f(&self.validation.lock())
    }
}
