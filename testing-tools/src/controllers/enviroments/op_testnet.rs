use std::pin::Pin;

use alloy::{
    node_bindings::AnvilInstance,
    primitives::Address,
    providers::{WalletProvider as _, ext::AnvilApi},
    signers::local::PrivateKeySigner
};
use angstrom_types::{primitive::AngstromSigner, testnet::InitialTestnetState};
use futures::{Future, FutureExt};
use reth_chainspec::Hardforks;
use reth_provider::{BlockReader, ChainSpecProvider, HeaderProvider, ReceiptProvider};
use reth_tasks::TaskExecutor;

use crate::{
    agents::AgentConfig,
    controllers::strom::OpTestnetNode,
    providers::{AnvilInitializer, AnvilProvider, WalletProvider},
    types::{
        WithWalletProvider,
        config::{OpTestnetConfig, TestingNodeConfig},
        initial_state::PartialConfigPoolKey
    }
};

pub struct OpAngstromTestnet {
    pub node:            OpTestnetNode<WalletProvider, OpTestnetConfig>,
    pub _anvil_instance: AnvilInstance
}

impl OpAngstromTestnet {
    pub async fn spawn_testnet<C, F>(
        _c: C,
        config: OpTestnetConfig,
        agents: Vec<F>,
        ex: TaskExecutor
    ) -> eyre::Result<Self>
    where
        C: BlockReader<Block = reth_primitives::Block>
            + ReceiptProvider<Receipt = reth_primitives::Receipt>
            + HeaderProvider<Header = reth_primitives::Header>
            + ChainSpecProvider<ChainSpec: Hardforks>
            + Unpin
            + Clone
            + 'static,
        F: for<'a> Fn(
            &'a InitialTestnetState,
            AgentConfig
        ) -> Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + 'a>>,
        F: Clone
    {
        tracing::info!("initializing op-testnet");

        let node_config = TestingNodeConfig::new(0, config, 100);
        let node_addresses = vec![node_config.angstrom_signer().address()];
        let pool_keys = node_config.pool_keys();

        // Initialize provider and deploy contracts
        let provider = Self::spawn_provider(node_config.clone(), node_addresses).await?;
        let (anvil_instance, provider, initial_state) =
            Self::anvil_deployment(provider, pool_keys, ex.clone()).await?;

        // Create single testnet node
        let node = OpTestnetNode::new(node_config, provider, initial_state, agents, ex).await?;

        Ok(Self { node, _anvil_instance: anvil_instance })
    }

    pub async fn run_to_completion(self) -> eyre::Result<()> {
        tracing::info!("running single node to completion");
        self.node.testnet_future().await;
        Ok(())
    }

    pub fn state_provider(&self) -> &AnvilProvider<WalletProvider> {
        self.node.state_provider()
    }

    pub fn get_sk(&self) -> AngstromSigner<PrivateKeySigner> {
        self.node.get_sk()
    }

    /// Signal the node to shutdown gracefully.
    pub fn shutdown(&self) {
        self.node.shutdown();
    }

    /// Cloneable shutdown sender for use in tests/tasks to request shutdown
    /// without needing ownership of the whole struct.
    pub fn shutdown_sender(&self) -> tokio::sync::watch::Sender<bool> {
        self.node.shutdown_sender()
    }

    async fn spawn_provider(
        node_config: TestingNodeConfig<OpTestnetConfig>,
        node_addresses: Vec<Address>
    ) -> eyre::Result<AnvilProvider<AnvilInitializer>> {
        AnvilProvider::from_future(
            AnvilInitializer::new(node_config.clone(), node_addresses)
                .then(async |v| v.map(|i| (i.0, i.1, Some(i.2)))),
            true
        )
        .await
    }

    pub async fn anvil_deployment(
        mut provider: AnvilProvider<AnvilInitializer>,
        pool_keys: Vec<PartialConfigPoolKey>,
        ex: TaskExecutor
    ) -> eyre::Result<(AnvilInstance, AnvilProvider<WalletProvider>, InitialTestnetState)> {
        let instance = provider._instance.take().unwrap();

        tracing::debug!(leader_address = ?provider.rpc_provider().default_signer_address());

        let initializer = provider.provider_mut().provider_mut();
        initializer.deploy_pool_fulls(pool_keys).await?;

        let initial_state = initializer.initialize_state_no_bytes(ex).await?;
        initializer
            .rpc_provider()
            .anvil_mine(Some(10), None)
            .await?;

        Ok((instance, provider.into_state_provider(), initial_state))
    }
}
