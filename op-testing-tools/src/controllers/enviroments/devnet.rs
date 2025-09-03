use std::pin::Pin;

use alloy::providers::ext::AnvilApi;
use alloy_primitives::U256;
use angstrom_types::testnet::InitialTestnetState;
use futures::{Future, FutureExt};
use reth_tasks::TaskExecutor;

use super::OpAngstromTestnet;
use crate::{
    agents::AgentConfig,
    controllers::{enviroments::DevnetStateMachine, strom::OpTestnetNode},
    providers::{AnvilInitializer, AnvilProvider, TestnetBlockProvider, WalletProvider},
    types::{
        GlobalTestingConfig,
        config::{DevnetConfig, TestingNodeConfig},
        initial_state::{Erc20ToDeploy, PartialConfigPoolKey}
    }
};

impl OpAngstromTestnet<DevnetConfig, WalletProvider> {
    pub async fn spawn_devnet(config: DevnetConfig, ex: TaskExecutor) -> eyre::Result<Self> {
        let block_provider = TestnetBlockProvider::new();
        let mut this = Self {
            peers: Default::default(),
            block_syncs: vec![],
            current_max_peer_id: 0,
            config: config.clone(),
            block_provider,
            _anvil_instance: None
        };

        tracing::info!("initializing devnet with {} nodes", config.node_count());
        this.spawn_new_devnet_nodes(ex).await?;
        tracing::info!("initialization devnet with {} nodes", config.node_count());

        Ok(this)
    }

    pub fn as_state_machine<'a>(self) -> DevnetStateMachine<'a> {
        DevnetStateMachine::new(self)
    }

    async fn spawn_new_devnet_nodes(&mut self, ex: TaskExecutor) -> eyre::Result<()> {
        #[allow(unused_assignments)]
        let mut initial_angstrom_state = None;

        let config = TestingNodeConfig::new(0, self.config.clone(), 100);
        let node_address = config.angstrom_signer().address();
        let node_id = config.node_id;

        let provider = {
            let mut initializer = AnvilProvider::from_future(
                AnvilInitializer::new(config.clone(), vec![node_address])
                    .then(async |v| v.map(|i| (i.0, i.1, Some(i.2)))),
                false
            )
            .await?;

            let provider = initializer.provider_mut().provider_mut();

            initial_angstrom_state = Some(provider.initialize_state(ex.clone()).await?);
            tracing::info!(?initial_angstrom_state, "initialized angstrom state");

            initializer.rpc_provider().anvil_mine(Some(5), None).await?;
            initializer.into_state_provider()
        };

        let _node = OpTestnetNode::new(
            config,
            provider,
            initial_angstrom_state.clone().unwrap(),
            vec![a],
            ex.clone()
        )
        .await?;
        tracing::info!(node_id, "made angstrom node");

        Ok(())
    }

    /// deploys a new pool
    pub async fn deploy_new_pool(
        &self,
        pool_key: PartialConfigPoolKey,
        token0: Erc20ToDeploy,
        token1: Erc20ToDeploy,
        store_index: U256
    ) -> eyre::Result<()> {
        tracing::debug!("deploying new pool on state machine");
        let node = self.get_peer_with(|n| n.state_provider().deployed_addresses().is_some());
        let provider = node.state_provider();
        let config = node.testnet_node_config();

        let mut initializer = AnvilInitializer::new_existing(provider, config);
        initializer
            .deploy_extra_pool_full(pool_key, token0, token1, store_index)
            .await?;

        Ok(())
    }
}

fn a<'a>(
    _: &'a InitialTestnetState,
    _: AgentConfig
) -> Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + 'a>> {
    Box::pin(async { eyre::Ok(()) })
}
