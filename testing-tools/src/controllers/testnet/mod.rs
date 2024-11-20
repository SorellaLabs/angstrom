mod config;

use angstrom_types::testnet::InitialTestnetState;
pub use config::*;
use consensus::AngstromValidator;
use reth_chainspec::Hardforks;
use reth_provider::{BlockReader, ChainSpecProvider, HeaderProvider};

use super::strom::initialize_new_node;
use crate::{
    anvil_state_provider::{AnvilInitializer, TestnetBlockProvider},
    controllers::strom::TestnetNode
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
        // let mut initializer = AnvilInitializer::new(config.clone()).await?;
        // initializer.deploy_pool_full().await?;
        // let initial_state = initializer.initialize_state().await?;

        let block_provider = TestnetBlockProvider::new();
        let (leader_handle, inital_angstrom_state) = if config.is_leader() {
            let mut initializer = AnvilInitializer::new(config.clone()).await?;
            initializer.deploy_pool_full().await?;

            let init_state = initializer.initialize_state().await?;

            (Some(initializer), init_state)
        } else {
            let init_state =
                InitialTestnetState::new(config.angstrom_address, config.pool_manager_address, None, config.pool_keys.clone());
            (None, init_state)
        };

        let node = initialize_new_node(
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

        Ok(Self { block_provider, node, leader_handle, config })
    }
}
