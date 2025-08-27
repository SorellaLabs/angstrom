use std::pin::Pin;

use alloy::signers::local::PrivateKeySigner;
use angstrom_types::{primitive::AngstromSigner, testnet::InitialTestnetState};
use futures::Future;
use reth_tasks::TaskExecutor;

use crate::{
    agents::AgentConfig,
    providers::{AnvilProvider, WalletProvider},
    types::{GlobalTestingConfig, WithWalletProvider, config::TestingNodeConfig}
};

/// Minimal OP testnet node: no custom networking or consensus.
pub struct OpTestnetNode<P, G> {
    state_provider: AnvilProvider<P>,
    init_state:     InitialTestnetState,
    config:         TestingNodeConfig<G>
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
        _agents: Vec<F>,
        _ex: TaskExecutor
    ) -> eyre::Result<Self>
    where
        F: for<'a> Fn(
                &'a InitialTestnetState,
                AgentConfig
            ) -> Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + 'a>>
            + Clone
    {
        Ok(Self { state_provider, init_state: inital_angstrom_state, config: node_config })
    }

    pub fn state_provider(&self) -> &AnvilProvider<P> {
        &self.state_provider
    }

    pub fn get_sk(&self) -> AngstromSigner<PrivateKeySigner> {
        self.config.angstrom_signer()
    }

    pub async fn testnet_future(self) {
        // Keep the node alive (no networking/consensus to drive here)
        futures::future::pending::<()>().await;
    }
}

// Convenience alias for OP Angstrom use-site
pub type OpWalletNode<G> = OpTestnetNode<WalletProvider, G>;

