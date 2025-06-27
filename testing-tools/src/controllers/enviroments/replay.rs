use std::{
    collections::{HashMap, HashSet, VecDeque},
    time::Duration
};

use alloy::{
    node_bindings::AnvilInstance,
    primitives::Address,
    providers::{Provider, WalletProvider as _, ext::AnvilApi}
};
use angstrom_types::{
    block_sync::GlobalBlockSync,
    consensus::ConsensusRoundName,
    pair_with_price::PairsWithPrice,
    primitive::{ANGSTROM_ADDRESS, CONTROLLER_V1_ADDRESS, POOL_MANAGER_ADDRESS, PoolId},
    testnet::InitialTestnetState
};
use futures::FutureExt;
use order_pool::OrderPoolHandle;
use reth_chainspec::Hardforks;
use reth_provider::{BlockReader, ChainSpecProvider, HeaderProvider, ReceiptProvider};
use reth_tasks::{TaskExecutor, TaskSpawner};
use telemetry::blocklog::BlockLog;
use telemetry_recorder::TelemetryMessage;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_stream::{StreamExt, wrappers::UnboundedReceiverStream};

use super::AngstromTestnet;
use crate::{
    controllers::strom::TestnetNode,
    providers::{
        AnvilInitializer, AnvilProvider, AnvilStateProvider, TestnetBlockProvider, WalletProvider
    },
    types::{
        GlobalTestingConfig, WithWalletProvider,
        config::{ReplayConfig, TestingNodeConfig},
        initial_state::{DeployedAddresses, PartialConfigPoolKey}
    },
    utils::noop_agent
};

impl<C> AngstromTestnet<C, ReplayConfig, WalletProvider>
where
    C: BlockReader<Block = reth_primitives::Block>
        + ReceiptProvider<Receipt = reth_primitives::Receipt>
        + HeaderProvider<Header = reth_primitives::Header>
        + ChainSpecProvider<ChainSpec: Hardforks>
        + Unpin
        + Clone
        + 'static
{
    pub async fn replay_stuff(
        mut self,
        executor: TaskExecutor,
        replay_log: BlockLog,
        state_rx: UnboundedReceiver<ConsensusRoundName>
    ) -> eyre::Result<()> {
        let node = self.peers.remove(&0).unwrap();
        let provider = node.state_provider();
        let blocknum = provider.rpc_provider().get_block_number().await?;
        tracing::warn!(blocknum, "Block number before executing");

        let replay_log = replay_log.at_block(blocknum);

        let pool_handle = node.pool_handle();

        let consensus_sender = node.strom_tx_handles().consensus_tx_op;

        executor.spawn_critical(
            format!("testnet node {}", node.testnet_node_id()).leak(),
            Box::pin(node.testnet_future())
        );

        let mut state_stream = UnboundedReceiverStream::new(state_rx);

        // Playback our events in order
        for event in replay_log.events() {
            match event {
                TelemetryMessage::NewBlock { .. } => (),
                TelemetryMessage::NewOrder { origin, order, .. } => {
                    tracing::info!("NewOrder event playing back");
                    let res = pool_handle.new_order(*origin, order.clone()).await;
                    tracing::debug!(?res, "NewOrder result");
                }
                TelemetryMessage::CancelOrder { cancel, .. } => {
                    tracing::info!("CancelOrder event playing back");
                    let _res = pool_handle.cancel_order(cancel.clone()).await;
                }
                TelemetryMessage::Consensus { event, .. } => {
                    tracing::info!("Consensus event playing back");
                    let _res = consensus_sender.send(event.clone());
                }
                TelemetryMessage::ConsensusStateChange { state, .. } => {
                    tracing::info!("ConsensusStateChange event playing back");
                    // Wait for the new state to show up as it should
                    if let Some(new_state) = state_stream.next().await {
                        assert_eq!(*state, new_state, "Consensus state mismatch")
                    }
                }
                TelemetryMessage::GasPriceSnapshot { .. } => {
                    println!("Gas price snapshot not a valid replay event");
                }
                TelemetryMessage::Error { message, .. } => {
                    println!("Error: {message}");
                }

                _ => todo!()
            }
        }
        tracing::error!("Done with everything");
        while let Some(x) = state_stream.next().await {
            tracing::info!(state = ?x, "State transition received");
            if matches!(x, ConsensusRoundName::BidAggregation) {
                tracing::info!("Completed one block");
                break;
            }
        }

        Ok(())
    }

    pub async fn spawn_replay(
        c: C,
        config: ReplayConfig,
        ex: TaskExecutor,
        token_price_snapshot: Option<(HashMap<PoolId, VecDeque<PairsWithPrice>>, u128)>
    ) -> eyre::Result<(Self, UnboundedReceiver<ConsensusRoundName>)> {
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
        let state_rx = this.spawn_replay_node(c, ex, token_price_snapshot).await?;
        tracing::info!("initialized testnet with {} nodes", config.node_count());

        Ok((this, state_rx))
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

    async fn spawn_replay_node(
        &mut self,
        c: C,
        ex: TaskExecutor,
        token_price_snapshot: Option<(HashMap<PoolId, VecDeque<PairsWithPrice>>, u128)>
    ) -> eyre::Result<UnboundedReceiver<ConsensusRoundName>> {
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
        let node_config = configs.first().cloned().unwrap();
        let pool_keys = node_config.pool_keys();
        let (i, leader_provider, initial_angstrom_state) =
            if node_config.global_config.use_testnet() {
                // If we're using Testnet thene we want to deploy and configure a local testnet
                // Anvil
                let initializer_provider =
                    Self::spawn_provider(node_config.clone(), node_addresses).await?;
                tokio::time::sleep(Duration::from_millis(1000)).await;
                Self::anvil_deployment(initializer_provider, pool_keys, ex.clone()).await?
            } else {
                // If we're not using testnet, then we want to spawn a mainnet fork instead
                // This is experimental and I'm not sure how well any of it works yet
                let deployed_addresses =
                    DeployedAddresses::from_globals(Address::random(), Address::random());
                let res =
                    Self::spawn_mainnet_fork(node_config.clone(), Some(deployed_addresses)).await?;
                let state = InitialTestnetState::new(
                    *ANGSTROM_ADDRESS.get().unwrap(),
                    *CONTROLLER_V1_ADDRESS.get().unwrap(),
                    *POOL_MANAGER_ADDRESS.get().unwrap(),
                    None,
                    vec![],
                    ex.clone()
                );
                (res.0, res.1, state)
            };

        self._anvil_instance = Some(i);

        tracing::info!("connected to state provider");

        let node_pk = node_config.clone().angstrom_signer().id();
        let block_sync =
            GlobalBlockSync::new(leader_provider.rpc_provider().get_block_number().await?);

        let (state_tx, state_rx) = tokio::sync::mpsc::unbounded_channel();
        let node = TestnetNode::new(
            c,
            node_config,
            leader_provider,
            initial_validators,
            initial_angstrom_state,
            vec![noop_agent],
            block_sync.clone(),
            ex.clone(),
            Some(state_tx),
            token_price_snapshot
        )
        .await?;
        tracing::info!(?node_pk, "node pk!!!!!!!!!!!");

        tracing::info!("made angstrom node");

        // node.connect_to_all_peers(&mut self.peers).await;
        tracing::info!("connected node");
        self.peers.insert(0, node);

        Ok(state_rx)
    }

    async fn spawn_provider(
        node_config: TestingNodeConfig<ReplayConfig>,
        node_addresses: Vec<Address>
    ) -> eyre::Result<AnvilProvider<AnvilInitializer>> {
        tracing::warn!("Spawning anvil provider");
        AnvilProvider::from_future(
            AnvilInitializer::new(node_config.clone(), node_addresses)
                .then(async |v| v.map(|i| (i.0, i.1, Some(i.2)))),
            true
        )
        .await
    }

    async fn spawn_mainnet_fork(
        config: TestingNodeConfig<ReplayConfig>,
        deployed_addresses: Option<DeployedAddresses>
    ) -> eyre::Result<(AnvilInstance, AnvilProvider<WalletProvider>)> {
        let (wallet_provider, anvil) = config.spawn_anvil_rpc().await?;
        let provider = AnvilStateProvider::new(wallet_provider);
        let mut ap = AnvilProvider::new(provider, anvil, deployed_addresses);
        let instance = ap._instance.take().unwrap();
        Ok((instance, ap))
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
