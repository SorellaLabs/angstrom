//! CLI definition and entrypoint to executable

use std::{collections::HashSet, pin::Pin, sync::Arc, time::Duration};

use alloy::{
    self,
    eips::{BlockId, BlockNumberOrTag},
    primitives::Address,
    providers::{Provider, ProviderBuilder, network::Ethereum}
};
use alloy_chains::Chain;
use angstrom_eth::{
    handle::{Eth, EthCommand},
    manager::{EthDataCleanser, EthEvent}
};
use angstrom_network::{
    NetworkBuilder as StromNetworkBuilder, NetworkOrderEvent, PoolManagerBuilder, StatusState,
    VerificationSidecar,
    manager::StromConsensusEvent,
    pool_manager::{OrderCommand, PoolHandle}
};
use angstrom_types::{
    block_sync::{BlockSyncProducer, GlobalBlockSync},
    contract_payloads::angstrom::{AngstromPoolConfigStore, UniswapAngstromRegistry},
    mev_boost::MevBoostProvider,
    primitive::{AngstromSigner, UniswapPoolRegistry},
    reth_db_provider::RethDbLayer,
    reth_db_wrapper::RethDbWrapper
};
use consensus::{AngstromValidator, ConsensusManager, ManagerNetworkDeps};
use futures::Stream;
use matching_engine::{MatchingManager, manager::MatcherCommand};
use order_pool::{PoolConfig, PoolManagerUpdate, order_storage::OrderStorage};
use parking_lot::RwLock;
use reth::{
    api::NodeAddOns,
    builder::FullNodeComponents,
    chainspec::ChainSpec,
    core::exit::NodeExitFuture,
    primitives::EthPrimitives,
    providers::{BlockNumReader, CanonStateNotification, CanonStateSubscriptions},
    tasks::TaskExecutor
};
use reth_metrics::common::mpsc::{UnboundedMeteredReceiver, UnboundedMeteredSender};
use reth_network::{NetworkHandle, Peers};
use reth_node_builder::{FullNode, NodeTypes, node::FullNodeTypes, rpc::RethRpcAddOns};
use reth_provider::{
    BlockReader, DatabaseProviderFactory, ReceiptProvider, TryIntoHistoricalStateProvider
};
use tokio::sync::mpsc::{
    Receiver, Sender, UnboundedReceiver, UnboundedSender, channel, unbounded_channel
};
use uniswap_v4::{configure_uniswap_manager, fetch_angstrom_pools};
use validation::{
    common::TokenPriceGenerator,
    init_validation,
    validator::{ValidationClient, ValidationRequest}
};

use crate::{AngstromConfig, cli::NodeConfig};

pub fn init_network_builder(
    secret_key: AngstromSigner,
    eth_handle: UnboundedReceiver<EthEvent>,
    validator_set: Arc<RwLock<HashSet<Address>>>
) -> eyre::Result<StromNetworkBuilder<NetworkHandle>> {
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

pub type DefaultPoolHandle = PoolHandle;
type DefaultOrderCommand = OrderCommand;

// due to how the init process works with reth. we need to init like this
pub struct StromHandles {
    pub eth_tx: Sender<EthCommand>,
    pub eth_rx: Receiver<EthCommand>,

    pub pool_tx: UnboundedMeteredSender<NetworkOrderEvent>,
    pub pool_rx: UnboundedMeteredReceiver<NetworkOrderEvent>,

    pub orderpool_tx: UnboundedSender<DefaultOrderCommand>,
    pub orderpool_rx: UnboundedReceiver<DefaultOrderCommand>,

    pub validator_tx: UnboundedSender<ValidationRequest>,
    pub validator_rx: UnboundedReceiver<ValidationRequest>,

    pub eth_handle_tx: Option<UnboundedSender<EthEvent>>,
    pub eth_handle_rx: Option<UnboundedReceiver<EthEvent>>,

    pub pool_manager_tx: tokio::sync::broadcast::Sender<PoolManagerUpdate>,

    pub consensus_tx_op: UnboundedMeteredSender<StromConsensusEvent>,
    pub consensus_rx_op: UnboundedMeteredReceiver<StromConsensusEvent>,

    // only 1 set cur
    pub matching_tx: Sender<MatcherCommand>,
    pub matching_rx: Receiver<MatcherCommand>
}

impl StromHandles {
    pub fn get_pool_handle(&self) -> DefaultPoolHandle {
        PoolHandle {
            manager_tx:      self.orderpool_tx.clone(),
            pool_manager_tx: self.pool_manager_tx.clone()
        }
    }
}

pub fn initialize_strom_handles() -> StromHandles {
    let (eth_tx, eth_rx) = channel(100);
    let (matching_tx, matching_rx) = channel(100);
    let (pool_manager_tx, _) = tokio::sync::broadcast::channel(100);
    let (pool_tx, pool_rx) = reth_metrics::common::mpsc::metered_unbounded_channel("orderpool");
    let (orderpool_tx, orderpool_rx) = unbounded_channel();
    let (validator_tx, validator_rx) = unbounded_channel();
    let (eth_handle_tx, eth_handle_rx) = unbounded_channel();
    let (consensus_tx_op, consensus_rx_op) =
        reth_metrics::common::mpsc::metered_unbounded_channel("orderpool");

    StromHandles {
        eth_tx,
        eth_rx,
        pool_tx,
        pool_rx,
        orderpool_tx,
        orderpool_rx,
        validator_tx,
        validator_rx,
        pool_manager_tx,
        consensus_tx_op,
        consensus_rx_op,
        matching_tx,
        matching_rx,
        eth_handle_tx: Some(eth_handle_tx),
        eth_handle_rx: Some(eth_handle_rx)
    }
}

pub async fn initialize_strom_components<Node, AddOns, P: Peers + Unpin + 'static>(
    config: AngstromConfig,
    signer: AngstromSigner,
    mut handles: StromHandles,
    network_builder: StromNetworkBuilder<P>,
    node: &FullNode<Node, AddOns>,
    executor: TaskExecutor,
    exit: NodeExitFuture,
    node_set: HashSet<Address>,
    node_config: NodeConfig
) -> eyre::Result<()>
where
    Node: FullNodeComponents
        + FullNodeTypes<Types: NodeTypes<ChainSpec = ChainSpec, Primitives = EthPrimitives>>,
    Node::Provider: BlockReader<
            Block = reth::primitives::Block,
            Receipt = reth::primitives::Receipt,
            Header = reth::primitives::Header
        > + DatabaseProviderFactory,
    AddOns: NodeAddOns<Node> + RethRpcAddOns<Node>,
    <<Node as FullNodeTypes>::Provider as DatabaseProviderFactory>::Provider:
        TryIntoHistoricalStateProvider + ReceiptProvider,
    <<Node as FullNodeTypes>::Provider as DatabaseProviderFactory>::Provider: BlockNumReader
{
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

    let mev_boost_provider =
        MevBoostProvider::new_from_urls(querying_provider.clone(), &config.mev_boost_endpoints);

    tracing::info!(target: "angstrom::startup-sequence", "waiting for the next block to continue startup sequence. \
        this is done to ensure all modules start on the same state and we don't hit the rare  \
        condition of a block while starting modules");

    // wait for the next block so that we have a full 12 seconds on startup.
    let mut sub = node.provider.subscribe_to_canonical_state();
    handle_init_block_spam(&mut sub).await;
    let _ = sub.recv().await.expect("next block");

    tracing::info!(target: "angstrom::startup-sequence", "new block detected. initializing all modules");

    let block_id = querying_provider.get_block_number().await.unwrap();

    let pool_config_store = Arc::new(
        AngstromPoolConfigStore::load_from_chain(
            node_config.angstrom_address,
            BlockId::Number(BlockNumberOrTag::Latest),
            &querying_provider
        )
        .await
        .unwrap()
    );

    // load the angstrom pools;
    tracing::info!("starting search for pools");
    let pools = fetch_angstrom_pools(
        node_config.angstrom_deploy_block as usize,
        block_id as usize,
        node_config.angstrom_address,
        &node.provider
    )
    .await;
    tracing::info!("found pools");

    // re-fetch given the fetch pools takes awhile. given this, we do techincally
    // have a gap in which a pool is deployed durning startup. This isn't
    // critical but we will want to fix this down the road.
    // let block_id = querying_provider.get_block_number().await.unwrap();
    let block_id = match sub.recv().await.expect("first block") {
        CanonStateNotification::Commit { new } => new.tip().number,
        CanonStateNotification::Reorg { new, .. } => new.tip().number
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
    let eth_handle = EthDataCleanser::spawn(
        node_config.angstrom_address,
        node_config.periphery_address,
        eth_data_sub,
        executor.clone(),
        handles.eth_tx,
        handles.eth_rx,
        HashSet::new(),
        pool_config_store.clone(),
        global_block_sync.clone(),
        node_set.clone(),
        vec![handles.eth_handle_tx.take().unwrap()]
    )
    .unwrap();

    let network_stream = Box::pin(eth_handle.subscribe_network())
        as Pin<Box<dyn Stream<Item = EthEvent> + Send + Sync>>;

    let uniswap_pool_manager = configure_uniswap_manager(
        querying_provider.clone(),
        eth_handle.subscribe_cannon_state_notifications().await,
        uniswap_registry,
        block_id,
        global_block_sync.clone(),
        node_config.pool_manager_address,
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
        node_config.gas_token_address,
        None
    )
    .await
    .expect("failed to start token price generator");

    let block_height = node.provider.best_block_number().unwrap();

    init_validation(
        RethDbWrapper::new(node.provider.clone()),
        block_height,
        node_config.angstrom_address,
        node_address,
        // Because this is incapsulated under the orderpool syncer. this is the only case
        // we can use the raw stream.
        node.provider.canonical_state_stream(),
        uniswap_pools.clone(),
        price_generator,
        pool_config_store.clone(),
        handles.validator_rx
    );

    let validation_handle = ValidationClient(handles.validator_tx.clone());
    tracing::info!("validation manager start");

    let network_handle = network_builder
        .with_pool_manager(handles.pool_tx)
        .with_consensus_manager(handles.consensus_tx_op)
        .build_handle(executor.clone(), node.provider.clone());

    // fetch pool ids

    let pool_config = PoolConfig::with_pool_ids(pool_ids);
    let order_storage = Arc::new(OrderStorage::new(&pool_config));

    let _pool_handle = PoolManagerBuilder::new(
        validation_handle.clone(),
        Some(order_storage.clone()),
        network_handle.clone(),
        eth_handle.subscribe_network(),
        handles.pool_rx,
        global_block_sync.clone()
    )
    .with_config(pool_config)
    .build_with_channels(
        executor.clone(),
        handles.orderpool_tx,
        handles.orderpool_rx,
        handles.pool_manager_tx,
        block_id
    );
    let validators = node_set
        .into_iter()
        // use same weight for all validators
        .map(|addr| AngstromValidator::new(addr, 100))
        .collect::<Vec<_>>();
    tracing::info!("pool manager start");

    // spinup matching engine
    let matching_handle = MatchingManager::spawn(executor.clone(), validation_handle.clone());

    let manager = ConsensusManager::new(
        ManagerNetworkDeps::new(
            network_handle.clone(),
            eth_handle.subscribe_cannon_state_notifications().await,
            handles.consensus_rx_op
        ),
        signer,
        validators,
        order_storage.clone(),
        block_height,
        node_config.angstrom_address,
        uni_ang_registry,
        uniswap_pools.clone(),
        mev_boost_provider,
        matching_handle,
        global_block_sync.clone()
    );

    executor.spawn_critical_with_graceful_shutdown_signal("consensus", move |grace| {
        manager.run_till_shutdown(grace)
    });

    global_block_sync.finalize_modules();
    tracing::info!("started angstrom");
    exit.await
}

async fn handle_init_block_spam(
    canon: &mut tokio::sync::broadcast::Receiver<CanonStateNotification>
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
