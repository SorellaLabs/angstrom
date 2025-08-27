use std::pin::Pin;

use alloy::{
    node_bindings::AnvilInstance,
    primitives::Address,
    providers::{WalletProvider as _, ext::AnvilApi}
};
use angstrom_types::{block_sync::GlobalBlockSync, testnet::InitialTestnetState};
use futures::{Future, FutureExt};
use reth_chainspec::Hardforks;
use reth_provider::{BlockReader, ChainSpecProvider, HeaderProvider, ReceiptProvider};
use reth_tasks::TaskExecutor;

use crate::{
    agents::AgentConfig,
    controllers::strom::TestnetNode,
    providers::{AnvilInitializer, AnvilProvider, WalletProvider},
    types::{
        WithWalletProvider,
        config::{OpTestnetConfig, TestingNodeConfig},
        initial_state::PartialConfigPoolKey
    }
};

pub struct OpAngstromTestnet<C: Unpin> {
    node:            TestnetNode<C, WalletProvider, OpTestnetConfig>,
    _anvil_instance: AnvilInstance
}

impl<C> OpAngstromTestnet<C>
where
    C: BlockReader<Block = reth_primitives::Block>
        + ReceiptProvider<Receipt = reth_primitives::Receipt>
        + HeaderProvider<Header = reth_primitives::Header>
        + ChainSpecProvider<ChainSpec: Hardforks>
        + Unpin
        + Clone
        + 'static
{
    pub async fn spawn_testnet<F>(
        c: C,
        config: OpTestnetConfig,
        agents: Vec<F>,
        ex: TaskExecutor
    ) -> eyre::Result<Self>
    where
        F: for<'a> Fn(
            &'a InitialTestnetState,
            AgentConfig
        ) -> Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + 'a>>,
        F: Clone
    {
        tracing::info!("initializing op-testnet");

        let node_config = TestingNodeConfig::new(0, config, 100);
        let node_addresses = vec![node_config.angstrom_signer().address()];
        let initial_validators = vec![node_config.angstrom_validator()];
        let pool_keys = node_config.pool_keys();

        // Initialize provider and deploy contracts
        let provider = Self::spawn_provider(node_config.clone(), node_addresses).await?;
        let (anvil_instance, provider, initial_state) =
            Self::anvil_deployment(provider, pool_keys, ex.clone()).await?;

        // Create single testnet node
        let block_sync = GlobalBlockSync::new(0);
        let node = TestnetNode::new(
            c,
            node_config,
            provider,
            initial_validators,
            initial_state,
            agents,
            block_sync,
            ex,
            None,
            None
        )
        .await?;

        Ok(Self { node, _anvil_instance: anvil_instance })
    }

    pub async fn run_to_completion(self) -> eyre::Result<()> {
        tracing::info!("running single node to completion");
        self.node.testnet_future().await;
        Ok(())
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
