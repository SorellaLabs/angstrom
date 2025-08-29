use std::{
    collections::{HashMap, HashSet},
    pin::Pin,
    str::FromStr,
    sync::Arc,
    time::Duration
};

use alloy::{
    self,
    consensus::{BlockHeader, Header},
    eips::{BlockId, BlockNumberOrTag},
    primitives::Address,
    providers::{Provider, ProviderBuilder, network::Ethereum}
};
use alloy_chains::Chain;
use angstrom_amm_quoter::{ConsensusQuoterManager, RollupQuoterManager};
use angstrom_eth::{
    handle::Eth,
    manager::{EthDataCleanser, EthEvent}
};
use angstrom_network::{NetworkBuilder as StromNetworkBuilder, StatusState, VerificationSidecar};
use angstrom_types::{
    block_sync::{BlockSyncProducer, GlobalBlockSync},
    contract_payloads::angstrom::{AngstromPoolConfigStore, UniswapAngstromRegistry},
    flashblocks::{FlashblocksRx, FlashblocksStream},
    pair_with_price::PairsWithPrice,
    primitive::{
        ANGSTROM_ADDRESS, ANGSTROM_DEPLOYED_BLOCK, AngstromMetaSigner, AngstromSigner,
        CONTROLLER_V1_ADDRESS, GAS_TOKEN_ADDRESS, POOL_MANAGER_ADDRESS, UniswapPoolRegistry
    },
    reth_db_provider::RethDbLayer,
    reth_db_wrapper::RethDbWrapper,
    submission::SubmissionHandler
};
use consensus::{AngstromValidator, ConsensusHandler, ConsensusManager, ManagerNetworkDeps};
use futures::Stream;
use matching_engine::MatchingManager;
use order_pool::{PoolConfig, order_storage::OrderStorage};
use parking_lot::RwLock;
use pool_manager::{consensus::ConsensusPoolManagerBuilder, rollup::RollupPoolManagerBuilder};
use reth::{
    api::NodeAddOns,
    builder::FullNodeComponents,
    chainspec::EthChainSpec,
    core::exit::NodeExitFuture,
    primitives::EthPrimitives,
    providers::{BlockNumReader, CanonStateNotification, CanonStateSubscriptions},
    tasks::TaskExecutor
};
use reth_network::NetworkHandle;
use reth_node_builder::{
    FullNode, NodeHandle, NodePrimitives, NodeTypes, node::FullNodeTypes, rpc::RethRpcAddOns
};
use reth_optimism_chainspec::{OpChainSpec, OpHardforks};
use reth_optimism_primitives::OpPrimitives;
use reth_provider::{
    BlockReader, BlockReaderIdExt, ChainSpecProvider, DatabaseProviderFactory, ReceiptProvider,
    StateProviderFactory, TryIntoHistoricalStateProvider
};
use serde::Serialize;
use telemetry::init_telemetry;
use tokio::sync::mpsc::UnboundedReceiver;
use uniswap_v4::{DEFAULT_TICKS, configure_uniswap_manager, fetch_angstrom_pools};
use url::Url;
use validation::{common::TokenPriceGenerator, init_validation, validator::ValidationClient};

use crate::{
    AngstromConfig,
    handles::{AngstromMode, ConsensusMode, RollupMode, StromHandles},
    manager::RollupManager
};

pub fn init_network_builder<S: AngstromMetaSigner>(
    secret_key: AngstromSigner<S>,
    eth_handle: UnboundedReceiver<EthEvent>,
    validator_set: Arc<RwLock<HashSet<Address>>>
) -> eyre::Result<StromNetworkBuilder<NetworkHandle, S>> {
    let public_key = secret_key.id();

    let state = StatusState {
        version:   0,
        chain:     Chain::mainnet().id(),
        peer:      public_key,
        timestamp: 0
    };

    let verification =
        VerificationSidecar { status: state, has_sent: false, has_received: false, secret_key };

    Ok(StromNetworkBuilder::new(verification, eth_handle, validator_set))
}

pub(crate) struct AngstromLauncher<N, AO, M, S>
where
    N: FullNodeComponents,
    N::Provider: BlockReader<
            Block = <M::Primitives as NodePrimitives>::Block,
            Receipt = <M::Primitives as NodePrimitives>::Receipt,
            Header = <M::Primitives as NodePrimitives>::BlockHeader
        > + DatabaseProviderFactory,
    AO: NodeAddOns<N> + RethRpcAddOns<N>,
    M: AngstromMode,
    S: AngstromMetaSigner
{
    config:           AngstromConfig,
    executor:         TaskExecutor,
    node:             FullNode<N, AO>,
    node_exit_future: NodeExitFuture,
    signer:           AngstromSigner<S>,
    handles:          StromHandles<M>,

    // Optional fields
    consensus_client: Option<ConsensusHandler>,
    network_builder:  Option<StromNetworkBuilder<N::Network, S>>,
    node_set:         Option<HashSet<Address>>,
    flashblocks:      Option<FlashblocksRx>
}

impl<N, AO, M, S> AngstromLauncher<N, AO, M, S>
where
    N: FullNodeComponents,
    N::Provider: BlockReader<
            Block = <M::Primitives as NodePrimitives>::Block,
            Receipt = <M::Primitives as NodePrimitives>::Receipt,
            Header = <M::Primitives as NodePrimitives>::BlockHeader
        > + DatabaseProviderFactory,
    AO: NodeAddOns<N> + RethRpcAddOns<N>,
    M: AngstromMode,
    S: AngstromMetaSigner
{
    pub fn new(
        config: AngstromConfig,
        executor: TaskExecutor,
        handle: NodeHandle<N, AO>,
        signer: AngstromSigner<S>,
        handles: StromHandles<M>
    ) -> Self {
        let NodeHandle { node, node_exit_future } = handle;

        Self {
            config,
            executor,
            node,
            node_exit_future,
            signer,
            handles,
            network_builder: None,
            consensus_client: None,
            node_set: None,
            flashblocks: None
        }
    }

    pub fn with_flashblocks(self, rx: FlashblocksRx) -> Self {
        Self { flashblocks: Some(rx), ..self }
    }
}

impl<N, AO, S> AngstromLauncher<N, AO, ConsensusMode, S>
where
    N: FullNodeComponents,
    N::Provider: BlockReader<
            Block = <EthPrimitives as NodePrimitives>::Block,
            Receipt = <EthPrimitives as NodePrimitives>::Receipt,
            Header = <EthPrimitives as NodePrimitives>::BlockHeader
        > + DatabaseProviderFactory,
    AO: NodeAddOns<N> + RethRpcAddOns<N>,
    <<N as FullNodeTypes>::Provider as DatabaseProviderFactory>::Provider:
        TryIntoHistoricalStateProvider + BlockNumReader + ReceiptProvider,
    S: AngstromMetaSigner,
    <<N as FullNodeTypes>::Types as NodeTypes>::Primitives: Serialize,
    <N as FullNodeTypes>::Types: NodeTypes<Primitives = EthPrimitives>
{
    /// Initialize the network builder with the node's network handle.
    pub fn with_network(self, network_builder: StromNetworkBuilder<N::Network, S>) -> Self {
        let network_builder = network_builder.with_reth(self.node.network.clone());
        Self { network_builder: Some(network_builder), ..self }
    }

    /// Initialize the consensus client.
    pub fn with_consensus_client(self, consensus_client: ConsensusHandler) -> Self {
        Self { consensus_client: Some(consensus_client), ..self }
    }

    /// Initialize the node set.
    pub fn with_node_set(self, node_set: HashSet<Address>) -> Self {
        Self { node_set: Some(node_set), ..self }
    }

    pub async fn launch(self) -> eyre::Result<()> {
        let signer = self.signer;
        let node = self.node;
        let config = self.config;
        let executor = self.executor;
        let mut handles = self.handles;
        let node_set = self.node_set.expect("node set configured in ConsensusMode");
        let network_builder = self
            .network_builder
            .expect("network builder configured in ConsensusMode");
        let consensus_client = self
            .consensus_client
            .expect("consensus client configured in ConsensusMode");

        let node_address = signer.address();

        // NOTE:
        // no key is installed and this is strictly for internal usage. Realsically, we
        // should build a alloy provider impl that just uses the raw underlying db
        // so it will be quicker than rpc + won't be bounded by the rpc threadpool.
        let url = node.rpc_server_handle().ipc_endpoint().unwrap();
        tracing::info!(?url, ?config.mev_boost_endpoints, "backup to database is");
        let querying_provider: Arc<_> = ProviderBuilder::<_, _, Ethereum>::default()
            .with_recommended_fillers()
            .layer(RethDbLayer::new(node.provider().clone()))
            // backup
            .connect(&url)
            .await
            .unwrap()
            .into();

        let angstrom_address = *ANGSTROM_ADDRESS.get().unwrap();
        let controller = *CONTROLLER_V1_ADDRESS.get().unwrap();
        let deploy_block = *ANGSTROM_DEPLOYED_BLOCK.get().unwrap();
        let gas_token = *GAS_TOKEN_ADDRESS.get().unwrap();
        let pool_manager = *POOL_MANAGER_ADDRESS.get().unwrap();

        let normal_nodes = config.get_normal_nodes();

        let angstrom_submission_nodes = config
            .angstrom_submission_nodes
            .into_iter()
            .map(|url| Url::from_str(&url).unwrap())
            .collect::<Vec<_>>();

        let mev_boost_endpoints = config
            .mev_boost_endpoints
            .into_iter()
            .map(|url| Url::from_str(&url).unwrap())
            .collect::<Vec<_>>();

        let submission_handler = SubmissionHandler::new(
            querying_provider.clone(),
            &normal_nodes,
            angstrom_address,
            signer.clone()
        )
        .with_angstrom(&angstrom_submission_nodes, angstrom_address, signer.clone())
        .with_mev_boost(&mev_boost_endpoints, signer.clone());

        tracing::info!(target: "angstrom::startup-sequence", "waiting for the next block to continue startup sequence. \
        this is done to ensure all modules start on the same state and we don't hit the rare  \
        condition of a block while starting modules");

        let mut sub = node.provider.subscribe_to_canonical_state();
        handle_init_block_spam(&mut sub).await;
        let _ = sub.recv().await.expect("next block");

        tracing::info!(target: "angstrom::startup-sequence", "new block detected. initializing all modules");

        let block_id = querying_provider.get_block_number().await.unwrap();

        let pool_config_store = Arc::new(
            AngstromPoolConfigStore::load_from_chain(
                angstrom_address,
                BlockId::Number(BlockNumberOrTag::Latest),
                &querying_provider
            )
            .await
            .unwrap()
        );

        // load the angstrom pools;
        tracing::info!("starting search for pools");
        let pools = fetch_angstrom_pools(
            deploy_block as usize,
            block_id as usize,
            angstrom_address,
            &node.provider
        )
        .await;
        tracing::info!("found pools");

        let angstrom_tokens = pools
            .iter()
            .flat_map(|pool| [pool.currency0, pool.currency1])
            .fold(HashMap::<Address, usize>::new(), |mut acc, x| {
                *acc.entry(x).or_default() += 1;
                acc
            });

        // re-fetch given the fetch pools takes awhile. given this, we do techincally
        // have a gap in which a pool is deployed durning startup. This isn't
        // critical but we will want to fix this down the road.
        // let block_id = querying_provider.get_block_number().await.unwrap();
        let block_id = match sub.recv().await.expect("first block") {
            CanonStateNotification::Commit { new } => new.tip().number(),
            CanonStateNotification::Reorg { new, .. } => new.tip().number()
        };

        tracing::info!(?block_id, "starting up with block");
        let eth_data_sub = node.provider.subscribe_to_canonical_state();

        let global_block_sync = GlobalBlockSync::new(block_id);

        // this right here problem
        let uniswap_registry: UniswapPoolRegistry = pools.into();
        let uni_ang_registry =
            UniswapAngstromRegistry::new(uniswap_registry.clone(), pool_config_store.clone());

        // Build our PoolManager using the PoolConfig and OrderStorage we've already
        // created
        let eth_handle = EthDataCleanser::<_, angstrom_eth::manager::ConsensusMode>::spawn(
            angstrom_address,
            controller,
            eth_data_sub,
            executor.clone(),
            handles.eth_tx,
            handles.eth_rx,
            angstrom_tokens,
            pool_config_store.clone(),
            global_block_sync.clone(),
            node_set.clone(),
            vec![handles.eth_handle_tx.take().unwrap()]
        )
        .unwrap();

        let network_stream = Box::pin(eth_handle.subscribe_network())
            as Pin<Box<dyn Stream<Item = EthEvent> + Send + Sync>>;

        let signer_addr = signer.address();
        executor.spawn_critical_with_graceful_shutdown_signal("telemetry init", |grace_shutdown| {
            init_telemetry(signer_addr, grace_shutdown)
        });

        let uniswap_pool_manager = configure_uniswap_manager::<_, EthPrimitives, DEFAULT_TICKS>(
            querying_provider.clone(),
            // eth_handle.subscribe_cannon_state_notifications().await,
            todo!(),
            uniswap_registry,
            block_id,
            global_block_sync.clone(),
            pool_manager,
            network_stream
        )
        .await;

        tracing::info!("uniswap manager start");

        let uniswap_pools = uniswap_pool_manager.pools();
        let pool_ids = uniswap_pool_manager.pool_addresses().collect::<Vec<_>>();

        executor.spawn_critical("uniswap pool manager", Box::pin(uniswap_pool_manager));
        let price_generator = TokenPriceGenerator::new(
            querying_provider.clone(),
            block_id,
            uniswap_pools.clone(),
            gas_token,
            None
        )
        .await
        .expect("failed to start token price generator");

        let update_stream = Box::pin(PairsWithPrice::into_price_update_stream(
            angstrom_address,
            node.provider.canonical_state_stream(),
            querying_provider.clone()
        ));

        let block_height = node.provider.best_block_number().unwrap();

        init_validation(
            RethDbWrapper::new(node.provider.clone(), block_height),
            block_height,
            angstrom_address,
            node_address,
            update_stream,
            uniswap_pools.clone(),
            price_generator,
            pool_config_store.clone(),
            handles.validator_rx
        );

        let validation_handle = ValidationClient(handles.validator_tx.clone());
        tracing::info!("validation manager start");

        let network_handle = network_builder
            .with_pool_manager(handles.pool_tx)
            .with_consensus_manager(handles.mode.consensus_tx_op)
            .build_handle(executor.clone(), node.provider.clone());

        // fetch pool ids

        let pool_config = PoolConfig::with_pool_ids(pool_ids);
        let order_storage = Arc::new(OrderStorage::new(&pool_config));

        let _pool_handle = ConsensusPoolManagerBuilder::new(
            validation_handle.clone(),
            Some(order_storage.clone()),
            network_handle.clone(),
            eth_handle.subscribe_network(),
            handles.pool_rx,
            global_block_sync.clone(),
            network_handle.subscribe_network_events(),
            Duration::from_millis(config.block_time_ms)
        )
        .with_config(pool_config)
        .build_with_channels(
            executor.clone(),
            handles.orderpool_tx,
            handles.orderpool_rx,
            handles.pool_manager_tx,
            block_id,
            |_| {}
        );
        let validators = node_set
            .into_iter()
            // use same weight for all validators
            .map(|addr| AngstromValidator::new(addr, 100))
            .collect::<Vec<_>>();
        tracing::info!("pool manager start");

        // spinup matching engine
        let matching_handle = MatchingManager::spawn(executor.clone(), validation_handle.clone());

        // spin up amm quoter
        let amm = ConsensusQuoterManager::new(
            global_block_sync.clone(),
            order_storage.clone(),
            handles.quoter_rx,
            uniswap_pools.clone(),
            rayon::ThreadPoolBuilder::default()
                .num_threads(6)
                .build()
                .expect("failed to build rayon thread pool"),
            Duration::from_millis(100),
            consensus_client.subscribe_consensus_round_event()
        );

        executor.spawn_critical("amm quoting service", amm);

        let manager = ConsensusManager::new(
            ManagerNetworkDeps::new(
                network_handle.clone(),
                // eth_handle.subscribe_cannon_state_notifications().await,
                todo!(),
                handles.mode.consensus_rx_op
            ),
            signer,
            validators,
            order_storage.clone(),
            deploy_block,
            block_height,
            uni_ang_registry,
            uniswap_pools.clone(),
            submission_handler,
            matching_handle,
            global_block_sync.clone(),
            handles.mode.consensus_rx_rpc,
            None,
            config.consensus_timing
        );

        executor.spawn_critical_with_graceful_shutdown_signal("consensus", move |grace| {
            manager.run_till_shutdown(grace)
        });

        global_block_sync.finalize_modules();
        tracing::info!("started angstrom");

        self.node_exit_future.await
    }
}

impl<N, AO, S> AngstromLauncher<N, AO, RollupMode, S>
where
    N: FullNodeComponents,
    N::Provider: BlockReader<
            Block = <OpPrimitives as NodePrimitives>::Block,
            Receipt = <OpPrimitives as NodePrimitives>::Receipt,
            Header = <OpPrimitives as NodePrimitives>::BlockHeader
        > + DatabaseProviderFactory
        + StateProviderFactory
        + ChainSpecProvider<ChainSpec: EthChainSpec<Header = Header> + OpHardforks>
        + BlockReaderIdExt<Header = Header>,
    AO: NodeAddOns<N> + RethRpcAddOns<N>,
    <<N as FullNodeTypes>::Provider as DatabaseProviderFactory>::Provider:
        TryIntoHistoricalStateProvider + BlockNumReader + ReceiptProvider,
    S: AngstromMetaSigner,
    <<N as FullNodeTypes>::Types as NodeTypes>::Primitives: Serialize,
    <N as FullNodeTypes>::Types: NodeTypes<Primitives = OpPrimitives>
{
    pub async fn launch(self) -> eyre::Result<()> {
        let signer = self.signer;
        let node = self.node;
        let config = self.config;
        let executor = self.executor;
        let mut handles = self.handles;

        let node_address = signer.address();

        // NOTE:
        // no key is installed and this is strictly for internal usage. Realsically, we
        // should build a alloy provider impl that just uses the raw underlying db
        // so it will be quicker than rpc + won't be bounded by the rpc threadpool.
        let url = node.rpc_server_handle().ipc_endpoint().unwrap();
        tracing::info!(?url, ?config.mev_boost_endpoints, "backup to database is");
        let querying_provider: Arc<_> = ProviderBuilder::<_, _, Ethereum>::default()
            .with_recommended_fillers()
            .layer(RethDbLayer::new(node.provider().clone()))
            // backup
            .connect(&url)
            .await
            .unwrap()
            .into();

        let angstrom_address = *ANGSTROM_ADDRESS.get().unwrap();
        let controller = *CONTROLLER_V1_ADDRESS.get().unwrap();
        let deploy_block = *ANGSTROM_DEPLOYED_BLOCK.get().unwrap();
        let gas_token = *GAS_TOKEN_ADDRESS.get().unwrap();
        let pool_manager = *POOL_MANAGER_ADDRESS.get().unwrap();

        let normal_nodes = config.get_normal_nodes();

        let submission_handler = SubmissionHandler::new(
            querying_provider.clone(),
            &normal_nodes,
            angstrom_address,
            signer.clone()
        );

        tracing::info!(target: "angstrom::startup-sequence", "waiting for the next block to continue startup sequence. \
        this is done to ensure all modules start on the same state and we don't hit the rare  \
        condition of a block while starting modules");

        let mut sub = node.provider.subscribe_to_canonical_state();
        handle_init_block_spam(&mut sub).await;
        let _ = sub.recv().await.expect("next block");

        tracing::info!(target: "angstrom::startup-sequence", "new block detected. initializing all modules");

        let block_id = querying_provider.get_block_number().await.unwrap();

        let pool_config_store = Arc::new(
            AngstromPoolConfigStore::load_from_chain(
                angstrom_address,
                BlockId::Number(BlockNumberOrTag::Latest),
                &querying_provider
            )
            .await
            .unwrap()
        );

        // load the angstrom pools;
        tracing::info!("starting search for pools");
        let pools = fetch_angstrom_pools(
            deploy_block as usize,
            block_id as usize,
            angstrom_address,
            &node.provider
        )
        .await;
        tracing::info!("found pools");

        let angstrom_tokens = pools
            .iter()
            .flat_map(|pool| [pool.currency0, pool.currency1])
            .fold(HashMap::<Address, usize>::new(), |mut acc, x| {
                *acc.entry(x).or_default() += 1;
                acc
            });

        // re-fetch given the fetch pools takes awhile. given this, we do techincally
        // have a gap in which a pool is deployed durning startup. This isn't
        // critical but we will want to fix this down the road.
        // let block_id = querying_provider.get_block_number().await.unwrap();
        let block_id = match sub.recv().await.expect("first block") {
            CanonStateNotification::Commit { new } => new.tip().number(),
            CanonStateNotification::Reorg { new, .. } => new.tip().number()
        };

        tracing::info!(?block_id, "starting up with block");
        let eth_data_sub = node.provider.subscribe_to_canonical_state();

        let global_block_sync = GlobalBlockSync::new(block_id);

        // this right here problem
        let uniswap_registry: UniswapPoolRegistry = pools.into();
        let uni_ang_registry =
            UniswapAngstromRegistry::new(uniswap_registry.clone(), pool_config_store.clone());

        let eth_handle = EthDataCleanser::<_, angstrom_eth::manager::RollupMode>::spawn(
            angstrom_address,
            controller,
            eth_data_sub,
            self.flashblocks.clone().map(FlashblocksStream::new),
            executor.clone(),
            handles.eth_tx,
            handles.eth_rx,
            angstrom_tokens,
            pool_config_store.clone(),
            global_block_sync.clone(),
            // Empty node set for rollup mode
            HashSet::new(),
            vec![handles.eth_handle_tx.take().unwrap()]
        )
        .unwrap();

        let network_stream = Box::pin(eth_handle.subscribe_network())
            as Pin<Box<dyn Stream<Item = EthEvent> + Send + Sync>>;

        let signer_addr = signer.address();
        executor.spawn_critical_with_graceful_shutdown_signal("telemetry init", |grace_shutdown| {
            init_telemetry(signer_addr, grace_shutdown)
        });

        let uniswap_pool_manager = configure_uniswap_manager::<_, OpPrimitives, DEFAULT_TICKS>(
            querying_provider.clone(),
            // eth_handle.subscribe_cannon_state_notifications().await,
            todo!(),
            uniswap_registry,
            block_id,
            global_block_sync.clone(),
            pool_manager,
            network_stream
        )
        .await;

        tracing::info!("uniswap manager start");

        let uniswap_pools = uniswap_pool_manager.pools();
        let pool_ids = uniswap_pool_manager.pool_addresses().collect::<Vec<_>>();

        executor.spawn_critical("uniswap pool manager", Box::pin(uniswap_pool_manager));
        let price_generator = TokenPriceGenerator::new(
            querying_provider.clone(),
            block_id,
            uniswap_pools.clone(),
            gas_token,
            None
        )
        .await
        .expect("failed to start token price generator");

        let update_stream = Box::pin(PairsWithPrice::into_price_update_stream(
            angstrom_address,
            node.provider.canonical_state_stream(),
            querying_provider.clone()
        ));

        let block_height = node.provider.best_block_number().unwrap();

        init_validation(
            RethDbWrapper::new(node.provider.clone(), block_height),
            block_height,
            angstrom_address,
            node_address,
            update_stream,
            uniswap_pools.clone(),
            price_generator,
            pool_config_store.clone(),
            handles.validator_rx
        );

        let validation_handle = ValidationClient(handles.validator_tx.clone());
        tracing::info!("validation manager start");

        // fetch pool ids

        let pool_config = PoolConfig::with_pool_ids(pool_ids);
        let order_storage = Arc::new(OrderStorage::new(&pool_config));

        let _pool_handle = RollupPoolManagerBuilder::new(
            validation_handle.clone(),
            Some(order_storage.clone()),
            eth_handle.subscribe_network(),
            global_block_sync.clone(),
            Duration::from_millis(config.block_time_ms)
        )
        .with_config(pool_config)
        .build_with_channels(
            executor.clone(),
            handles.orderpool_tx,
            handles.orderpool_rx,
            handles.pool_manager_tx,
            block_id,
            |_| {}
        );

        tracing::info!("pool manager start");

        // spinup matching engine
        let matching_handle = MatchingManager::spawn(executor.clone(), validation_handle.clone());

        // spin up amm quoter
        let amm = RollupQuoterManager::new(
            global_block_sync.clone(),
            order_storage.clone(),
            handles.quoter_rx,
            uniswap_pools.clone(),
            rayon::ThreadPoolBuilder::default()
                .num_threads(6)
                .build()
                .expect("failed to build rayon thread pool"),
            Duration::from_millis(100)
        );

        executor.spawn_critical("amm quoting service", amm);

        let driver = RollupManager::new(
            block_height,
            Duration::from_millis(config.block_time_ms),
            // BroadcastStream::new(eth_handle.subscribe_cannon_state_notifications().await),
            todo!(),
            global_block_sync.clone(),
            order_storage,
            uni_ang_registry,
            uniswap_pools,
            Arc::new(submission_handler),
            matching_handle,
            signer
        );

        executor.spawn_critical_with_graceful_shutdown_signal("rollup driver", move |grace| {
            driver.run_till_shutdown(grace)
        });

        global_block_sync.finalize_modules();
        tracing::info!("started angstrom");

        self.node_exit_future.await
    }
}

// TODO(mempirate): This isn't correct in rollup mode.
async fn handle_init_block_spam<N: NodePrimitives>(
    canon: &mut tokio::sync::broadcast::Receiver<CanonStateNotification<N>>
) {
    // wait for the first notification
    let _ = canon.recv().await.expect("first block");
    tracing::info!("got first block");

    loop {
        tokio::select! {
            // if we can go 10.5s without a update, we know that all of the pending cannon
            // state notifications have been processed and we are at the tip.
            _ = tokio::time::sleep(Duration::from_millis(1050)) => {
                break;
            }
            Ok(_) = canon.recv() => {

            }
        }
    }
    tracing::info!("finished handling block-spam");
}
