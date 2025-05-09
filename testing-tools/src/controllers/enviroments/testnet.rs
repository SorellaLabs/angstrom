use std::{cell::Cell, collections::HashSet, pin::Pin, rc::Rc};

use alloy::{
    primitives::Address,
    providers::{WalletProvider as _, ext::AnvilApi}
};
use angstrom_types::{block_sync::GlobalBlockSync, testnet::InitialTestnetState};
use futures::{Future, FutureExt, StreamExt};
use reth_chainspec::Hardforks;
use reth_provider::{BlockReader, ChainSpecProvider, HeaderProvider, ReceiptProvider};
use reth_tasks::{TaskExecutor, TaskSpawner};

use super::AngstromTestnet;
use crate::{
    agents::AgentConfig,
    controllers::strom::TestnetNode,
    providers::{AnvilInitializer, AnvilProvider, TestnetBlockProvider, WalletProvider},
    types::{
        GlobalTestingConfig, WithWalletProvider,
        config::{TestingNodeConfig, TestnetConfig}
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
    pub async fn spawn_testnet<F>(
        c: C,
        config: TestnetConfig,
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
        let block_provider = TestnetBlockProvider::new();
        let mut this = Self {
            peers: Default::default(),
            _disconnected_peers: HashSet::new(),
            _dropped_peers: HashSet::new(),
            current_max_peer_id: 0,
            config: config.clone(),
            block_provider,
            block_syncs: vec![],
            _anvil_instance: None
        };

        tracing::info!("initializing testnet with {} nodes", config.node_count());
        this.spawn_new_testnet_nodes(c, agents, ex).await?;
        tracing::info!("initialized testnet with {} nodes", config.node_count());

        Ok(this)
    }

    pub async fn run_to_completion<TP: TaskSpawner>(mut self, executor: TP) {
        for s in self.block_syncs {
            s.clear();
        }
        tracing::info!("cleared blocksyncs, run to cmp");

        let all_peers = std::mem::take(&mut self.peers).into_values().map(|peer| {
            executor.spawn_critical(
                format!("testnet node {}", peer.testnet_node_id()).leak(),
                Box::pin(peer.testnet_future())
            )
        });

        let _ = futures::future::select_all(all_peers).await;
    }

    async fn spawn_new_testnet_nodes<F>(
        &mut self,
        c: C,
        agents: Vec<F>,
        ex: TaskExecutor
    ) -> eyre::Result<()>
    where
        F: for<'a> Fn(
            &'a InitialTestnetState,
            AgentConfig
        ) -> Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + 'a>>,
        F: Clone
    {
        let mut initial_angstrom_state = None;

        let configs = (0..self.config.node_count())
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
        let front = configs.first().unwrap().clone();
        let leader_provider = Rc::new(Cell::new(
            self.initalize_leader_provider(
                front,
                &mut initial_angstrom_state,
                node_addresses,
                ex.clone()
            )
            .await?
        ));
        // take the provider and then set all people in the testnet as nodes.

        let nodes = futures::stream::iter(configs.into_iter())
            .map(|node_config| {
                let c = c.clone();
                let initial_validators = initial_validators.clone();
                let initial_angstrom_state = initial_angstrom_state.clone().unwrap();
                let agents = agents.clone();
                let leader_provider = leader_provider.clone();
                let ex = ex.clone();

                async move {
                    let node_id = node_config.node_id;
                    let block_sync = GlobalBlockSync::new(0);

                    tracing::info!(node_id, "connecting to state provider");
                    let provider = if node_id == 0 {
                        tracing::info!("replaced leader provider");
                        let mut config = node_config.clone();
                        // change so we don't spawn a new anvil instance
                        config.node_id = 69;

                        leader_provider.replace(
                            AnvilProvider::new(
                                WalletProvider::new(config)
                                    .then(async |v| v.map(|i| (i.0, i.1, None))),
                                true
                            )
                            .await?
                        )
                    } else {
                        tracing::info!(?node_id, "follower node init");
                        AnvilProvider::new(
                            WalletProvider::new(node_config.clone())
                                .then(async |v| v.map(|i| (i.0, i.1, None))),
                            true
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
                        agents.clone(),
                        block_sync.clone(),
                        ex.clone()
                    )
                    .await?;
                    tracing::info!(?node_pk, "node pk!!!!!!!!!!!");

                    tracing::info!(node_id, "made angstrom node");

                    eyre::Ok((node_id, node, block_sync))
                }
            })
            .buffer_unordered(100)
            .collect::<Vec<_>>()
            .await;

        for res in nodes {
            let (node_id, mut node, bs) = res?;
            bs.clear();

            node.connect_to_all_peers(&mut self.peers).await;
            self.peers.insert(node_id, node);
        }

        Ok(())
    }

    async fn initalize_leader_provider(
        &mut self,
        node_config: TestingNodeConfig<TestnetConfig>,
        initial_angstrom_state: &mut Option<InitialTestnetState>,
        node_addresses: Vec<Address>,
        ex: TaskExecutor
    ) -> eyre::Result<AnvilProvider<WalletProvider>> {
        let (p, initial_state) = self
            .leader_initialization(node_config.clone(), node_addresses, ex)
            .await?;
        *initial_angstrom_state = Some(initial_state);
        Ok(p)
    }

    async fn leader_initialization(
        &mut self,
        config: TestingNodeConfig<TestnetConfig>,
        node_addresses: Vec<Address>,
        ex: TaskExecutor
    ) -> eyre::Result<(AnvilProvider<WalletProvider>, InitialTestnetState)> {
        let mut provider = AnvilProvider::new(
            AnvilInitializer::new(config.clone(), node_addresses)
                .then(async |v| v.map(|i| (i.0, i.1, Some(i.2)))),
            true
        )
        .await?;
        self._anvil_instance = Some(provider._instance.take().unwrap());

        tracing::debug!(leader_address = ?provider.rpc_provider().default_signer_address());

        let initializer = provider.provider_mut().provider_mut();
        initializer.deploy_pool_fulls(config.pool_keys()).await?;

        let initial_state = initializer.initialize_state_no_bytes(ex).await?;
        initializer
            .rpc_provider()
            .anvil_mine(Some(10), None)
            .await?;

        Ok((provider.into_state_provider(), initial_state))
    }
}
