mod config;

use angstrom_types::testnet::InitialTestnetState;
pub use config::*;
use consensus::AngstromValidator;
use reth_chainspec::Hardforks;
use reth_provider::{BlockReader, ChainSpecProvider, HeaderProvider};

use super::strom::initialize_new_node;
use crate::{
    anvil_state_provider::{AnvilInitializer, TestnetBlockProvider},
    controllers::strom::TestnetNode,
    types::TestingConfig
};

pub struct AngstromTestnet<C> {
    block_provider: TestnetBlockProvider,
    node:           TestnetNode<C>,
    leader_handle:  Option<AnvilInitializer>,
    config:         TestnetConfig
}

impl<C> AngstromTestnet<C>
where
    C: BlockReader
        + HeaderProvider
        + ChainSpecProvider
        + Unpin
        + Clone
        + ChainSpecProvider<ChainSpec: Hardforks>
        + 'static
{
    pub async fn spawn_testnet(
        c: C,
        config: TestnetConfig,
        initial_validators: Vec<AngstromValidator>
    ) -> eyre::Result<Self> {
        if config.is_leader {
            Self::spawn_testnet_leader(c, config, initial_validators).await
        } else {
            Self::spawn_testnet_follower(c, config, initial_validators).await
        }
    }

    async fn spawn_testnet_leader(
        c: C,
        config: TestnetConfig,
        initial_validators: Vec<AngstromValidator>
    ) -> eyre::Result<Self> {
        let block_provider = TestnetBlockProvider::new();
        let mut initializer = AnvilInitializer::new(config.clone()).await?;
        initializer.deploy_pool_full().await?;

        let inital_angstrom_state = initializer.initialize_state().await?;

        let mut node = initialize_new_node(
            c,
            None,
            config.pk,
            config.secret_key.clone(),
            initial_validators,
            inital_angstrom_state,
            config.clone(),
            block_provider.subscribe_to_new_blocks()
        )
        .await?;

        node.start_network_and_consensus();

        node.initialize_internal_connections((config.node_count - 1) as usize)
            .await;

        Ok(Self { block_provider, node, leader_handle: Some(initializer), config })
    }

    async fn spawn_testnet_follower(
        c: C,
        config: TestnetConfig,
        initial_validators: Vec<AngstromValidator>
    ) -> eyre::Result<Self> {
        let block_provider = TestnetBlockProvider::new();
        let inital_angstrom_state = InitialTestnetState::new(
            config.angstrom_address,
            config.pool_manager_address,
            None,
            config.pool_keys.clone()
        );

        let mut node = initialize_new_node(
            c,
            None,
            config.pk,
            config.secret_key.clone(),
            initial_validators,
            inital_angstrom_state,
            config.clone(),
            block_provider.subscribe_to_new_blocks()
        )
        .await?;

        node.start_network_and_consensus();

        node.initialize_internal_connections((config.node_count - 1) as usize)
            .await;

        Ok(Self { block_provider, node, leader_handle: None, config })
    }

    pub async fn run(self) {
        self.node.testnet_future().await;
    }
}
