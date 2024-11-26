use std::collections::HashSet;

use reth_chainspec::Hardforks;
use reth_provider::{BlockReader, ChainSpecProvider, HeaderProvider};
use secp256k1::SecretKey;

use super::AngstromTestnet;
use crate::{
    controllers::strom::TestnetNode,
    providers::{AnvilInitializer, AnvilProvider, TestnetBlockProvider, WalletProvider},
    types::{
        config::{TestingNodeConfig, TestnetConfig},
        GlobalTestingConfig
    }
};

impl<C> AngstromTestnet<C, TestnetConfig, WalletProvider>
where
    C: BlockReader
        + HeaderProvider
        + ChainSpecProvider
        + Unpin
        + Clone
        + ChainSpecProvider<ChainSpec: Hardforks>
        + 'static
{
    pub async fn spawn_testnet(c: C, config: TestnetConfig) -> eyre::Result<Self> {
        let block_provider = TestnetBlockProvider::new();
        let mut this = Self {
            peers: Default::default(),
            _disconnected_peers: HashSet::new(),
            _dropped_peers: HashSet::new(),
            current_max_peer_id: 0,
            config: config.clone(),
            block_provider
        };

        tracing::info!("initializing testnet with {} nodes", config.node_count());
        this.spawn_new_testnet_nodes(c).await?;
        tracing::info!("initialization testnet with {} nodes", config.node_count());

        Ok(this)
    }

    async fn spawn_new_testnet_nodes(&mut self, c: C) -> eyre::Result<()> {
        let mut initial_angstrom_state = None;

        let configs = (0..self.config.node_count())
            .map(|_| {
                let node_id = self.incr_peer_id();
                let secret_key = SecretKey::new(&mut rand::thread_rng());
                TestingNodeConfig::new(node_id, self.config.clone(), secret_key, 100)
            })
            .collect::<Vec<_>>();

        let initial_validators = configs
            .iter()
            .map(|node_config| node_config.angstrom_validator())
            .collect::<Vec<_>>();

        for node_config in configs {
            let node_id = node_config.node_id;
            tracing::info!(node_id, "connecting to state provider");
            let provider = if self.config.is_leader(node_id) {
                let mut initializer =
                    AnvilProvider::new(AnvilInitializer::new(node_config.clone())).await?;
                let provider = initializer.provider_mut().provider_mut();
                provider.deploy_pool_full().await?;
                let initial_state = provider.initialize_state().await?;
                initial_angstrom_state = Some(initial_state);
                initializer.into_state_provider()
            } else {
                let state_bytes = initial_angstrom_state.clone().unwrap().state.unwrap();
                let provider = AnvilProvider::new(WalletProvider::new(node_config.clone())).await?;
                provider.set_state(state_bytes).await?;
                provider
            };

            tracing::info!(node_id, "connected to state provider");

            let mut node = TestnetNode::new(
                c.clone(),
                node_config,
                provider,
                initial_validators.clone(),
                initial_angstrom_state.clone().unwrap(),
                self.block_provider.subscribe_to_new_blocks()
            )
            .await?;
            tracing::info!(node_id, "made angstrom node");

            node.connect_to_all_peers(&mut self.peers).await;
            tracing::info!(node_id, "connected to all peers");

            self.peers.insert(node_id, node);
        }

        Ok(())
    }
}
