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
        tracing::info!("initializing devnet");

        let mut initial_angstrom_state = None;

        let config = TestingNodeConfig::new(0, config.clone(), 100);
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

            initializer.rpc_provider().anvil_mine(Some(5), None).await?;
            initializer.into_state_provider()
        };

        let node = OpTestnetNode::new(
            config.clone(),
            provider,
            initial_angstrom_state.clone().unwrap(),
            None,
            vec![a],
            ex.clone()
        )
        .await?;
        tracing::info!(node_id, "made angstrom node");

        Ok(Self {
            node,
            block_provider: TestnetBlockProvider::new(),
            config: config.global_config,
            anvil: None
        })
    }

    pub fn as_state_machine<'a>(self) -> DevnetStateMachine<'a> {
        DevnetStateMachine::new(self)
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
        let provider = self.node.state_provider();
        let config = self.node.testnet_node_config();

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
