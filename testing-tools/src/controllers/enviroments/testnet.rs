use std::{cell::Cell, collections::HashSet, pin::Pin, rc::Rc};

use alloy::{primitives::Address, providers::ext::AnvilApi};
use alloy_primitives::U256;
use angstrom_types::{block_sync::GlobalBlockSync, testnet::InitialTestnetState};
use futures::{Future, StreamExt};
use reth_chainspec::Hardforks;
use reth_provider::{BlockReader, ChainSpecProvider, HeaderProvider, ReceiptProvider};
use reth_tasks::TaskExecutor;

use super::AngstromTestnet;
use crate::{
    agents::AgentConfig,
    controllers::strom::TestnetNode,
    providers::{AnvilInitializer, AnvilProvider, TestnetBlockProvider, WalletProvider},
    types::{
        config::{TestingNodeConfig, TestnetConfig},
        GlobalTestingConfig, WithWalletProvider
    }
};

impl<C> AngstromTestnet<C, TestnetConfig, WalletProvider>
where
    C: BlockReader<Block = reth_primitives::Block>
        + ReceiptProvider<Receipt = reth_primitives::Receipt>
        + HeaderProvider<Header = reth_primitives::Header>
        + ChainSpecProvider<ChainSpec: Hardforks>
        + Unpin
        + Clone
        + 'static
{
    pub async fn spawn_testnet<F>(c: C, config: TestnetConfig, agents: Vec<F>) -> eyre::Result<Self>
    where
        F: for<'a> Fn(
            &'a InitialTestnetState,
            AgentConfig
        ) -> Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + 'a>>,
        F: Clone
    {
        let block_provider = TestnetBlockProvider::new();
        let mut this = Self {
            peers: Default::default(),
            _disconnected_peers: HashSet::new(),
            _dropped_peers: HashSet::new(),
            current_max_peer_id: 0,
            config: config.clone(),
            block_provider,
            _anvil_instance: None
        };

        tracing::info!("initializing testnet with {} nodes", config.node_count());
        this.spawn_new_testnet_nodes(c, agents).await?;
        tracing::info!("initialized testnet with {} nodes", config.node_count());

        Ok(this)
    }

    pub async fn run_to_completion(mut self, executor: TaskExecutor) {
        let all_peers = std::mem::take(&mut self.peers).into_values().map(|peer| {
            executor.spawn_critical_blocking(
                format!("testnet node {}", peer.testnet_node_id()).leak(),
                peer.testnet_future()
            )
        });

        let _ = futures::future::select_all(all_peers).await;
    }

    async fn spawn_new_testnet_nodes<F>(&mut self, c: C, agents: Vec<F>) -> eyre::Result<()>
    where
        F: for<'a> Fn(
            &'a InitialTestnetState,
            AgentConfig
        ) -> Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + 'a>>,
        F: Clone
    {
        let mut initial_angstrom_state = None;

        let mut configs = (0..self.config.node_count())
            .map(|_| {
                let node_id = self.incr_peer_id();
                TestingNodeConfig::new(node_id, self.config.clone(), 100)
            })
            .collect::<Vec<_>>();

        let initial_validators = configs
            .iter()
            .map(|node_config| node_config.angstrom_validator())
            .collect::<Vec<_>>();
        let node_addresses = configs
            .iter()
            .map(|c| c.angstrom_signer().address())
            .collect::<Vec<_>>();

        // initialize leader provider
        let front = configs.remove(0);
        let l_block_sync = GlobalBlockSync::new(0);
        let leader_provider = Rc::new(Cell::new(
            self.initalize_leader_provider(
                l_block_sync.clone(),
                front,
                &mut initial_angstrom_state,
                node_addresses
            )
            .await?
        ));
        // take the provider and then set all people in the testnet as nodes.

        let nodes = futures::stream::iter(configs.into_iter())
            .map(|node_config| {
                let block_s = l_block_sync.clone();
                let block_st = self.block_provider.subscribe_to_new_blocks();
                let c = c.clone();
                let initial_validators = initial_validators.clone();
                let initial_angstrom_state = initial_angstrom_state.clone().unwrap();
                let agents = agents.clone();
                let leader_provider = leader_provider.clone();

                async move {
                    let node_id = node_config.node_id;
                    let block_sync = if node_id == 0 { block_s } else { GlobalBlockSync::new(0) };

                    tracing::info!(node_id, "connecting to state provider");
                    let provider = if node_id == 0 {
                        leader_provider.replace(
                            AnvilProvider::new(
                                WalletProvider::new(node_config.clone()),
                                true,
                                block_sync.clone()
                            )
                            .await?
                        )
                    } else {
                        tracing::info!(?node_id, "follower node init");
                        AnvilProvider::new(
                            WalletProvider::new(node_config.clone()),
                            true,
                            block_sync.clone()
                        )
                        .await?
                    };

                    tracing::info!(node_id, "connected to state provider");

                    let node_pk = node_config.angstrom_signer().id();

                    let node = TestnetNode::new(
                        c,
                        node_config,
                        provider,
                        initial_validators,
                        initial_angstrom_state,
                        block_st,
                        agents.clone(),
                        block_sync
                    )
                    .await?;
                    tracing::info!(?node_pk, "node pk!!!!!!!!!!!");

                    tracing::info!(node_id, "made angstrom node");

                    eyre::Ok((node_id, node))
                }
            })
            .buffer_unordered(100)
            .collect::<Vec<_>>()
            .await;

        for res in nodes {
            let (node_id, mut node) = res?;
            node.connect_to_all_peers(&mut self.peers).await;
            self.peers.insert(node_id, node);
        }

        Ok(())
    }

    async fn initalize_leader_provider(
        &mut self,
        block_sync: GlobalBlockSync,
        node_config: TestingNodeConfig<TestnetConfig>,
        initial_angstrom_state: &mut Option<InitialTestnetState>,
        node_addresses: Vec<Address>
    ) -> eyre::Result<AnvilProvider<WalletProvider>> {
        let (p, initial_state) = self
            .leader_initialization(node_config.clone(), block_sync.clone(), node_addresses)
            .await?;
        *initial_angstrom_state = Some(initial_state);
        Ok(p)
    }

    async fn leader_initialization(
        &mut self,
        config: TestingNodeConfig<TestnetConfig>,
        block_sync: GlobalBlockSync,
        node_addresses: Vec<Address>
    ) -> eyre::Result<(AnvilProvider<WalletProvider>, InitialTestnetState)> {
        let mut provider = AnvilProvider::new(
            AnvilInitializer::new(config.clone(), node_addresses),
            true,
            block_sync
        )
        .await?;
        self._anvil_instance = Some(provider._instance.take().unwrap());

        let initializer = provider.provider_mut().provider_mut();
        initializer.deploy_pool_fulls(config.pool_keys()).await?;

        let initial_state = initializer.initialize_state_no_bytes().await?;
        initializer
            .rpc_provider()
            .anvil_mine(Some(U256::from(10)), None)
            .await?;

        Ok((provider.into_state_provider(), initial_state))
    }
}
