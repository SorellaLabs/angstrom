//! CLI definition and entrypoint to executable

use std::{collections::HashSet, sync::Arc};

use alloy::{
    self,
    eips::{BlockId, BlockNumberOrTag},
    providers::{network::Ethereum, Provider, ProviderBuilder}
};
use alloy_chains::Chain;
use angstrom_eth::{
    handle::{Eth, EthCommand},
    manager::{EthDataCleanser, EthEvent}
};
use angstrom_network::{
    manager::StromConsensusEvent,
    pool_manager::{OrderCommand, PoolHandle},
    NetworkBuilder as StromNetworkBuilder, NetworkOrderEvent, PoolManagerBuilder, StatusState,
    VerificationSidecar
};
use angstrom_types::{
    block_sync::{BlockSyncProducer, GlobalBlockSync},
    contract_bindings::controller_v_1::ControllerV1,
    contract_payloads::angstrom::{AngstromPoolConfigStore, UniswapAngstromRegistry},
    mev_boost::MevBoostProvider,
    primitive::{AngstromSigner, PeerId, UniswapPoolRegistry},
    reth_db_wrapper::RethDbWrapper
};
use consensus::{AngstromValidator, ConsensusManager, ManagerNetworkDeps};
use matching_engine::{configure_uniswap_manager, manager::MatcherCommand, MatchingManager};
use order_pool::{order_storage::OrderStorage, PoolConfig, PoolManagerUpdate};
use reth::{
    api::NodeAddOns,
    builder::FullNodeComponents,
    chainspec::ChainSpec,
    primitives::EthPrimitives,
    providers::{BlockNumReader, CanonStateSubscriptions},
    tasks::TaskExecutor
};
use reth_metrics::common::mpsc::{UnboundedMeteredReceiver, UnboundedMeteredSender};
use reth_node_builder::{node::FullNodeTypes, rpc::RethRpcAddOns, FullNode, NodeTypes};
use reth_provider::BlockReader;
use tokio::sync::mpsc::{
    channel, unbounded_channel, Receiver, Sender, UnboundedReceiver, UnboundedSender
};
use validation::{
    common::TokenPriceGenerator,
    init_validation,
    order::state::pools::AngstromPoolsTracker,
    validator::{ValidationClient, ValidationRequest}
};

use crate::{cli::NodeConfig, AngstromConfig};

pub fn init_network_builder(
    secret_key: AngstromSigner,
    eth_handle: UnboundedReceiver<EthEvent>
) -> eyre::Result<StromNetworkBuilder> {
    let public_key = secret_key.id();

    let state = StatusState {
        version:   0,
        chain:     Chain::mainnet().id(),
        peer:      public_key,
        timestamp: 0
    };

    let verification =
        VerificationSidecar { status: state, has_sent: false, has_received: false, secret_key };

    Ok(StromNetworkBuilder::new(verification, eth_handle))
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

pub async fn initialize_strom_components<Node, AddOns>(
    config: AngstromConfig,
    signer: AngstromSigner,
    mut handles: StromHandles,
    network_builder: StromNetworkBuilder,
    node: FullNode<Node, AddOns>,
    executor: &TaskExecutor
) where
    Node: FullNodeComponents
        + FullNodeTypes<Types: NodeTypes<ChainSpec = ChainSpec, Primitives = EthPrimitives>>,
    Node::Provider: BlockReader<
        Block = reth::primitives::Block,
        Receipt = reth::primitives::Receipt,
        Header = reth::primitives::Header
    >,
    AddOns: NodeAddOns<Node> + RethRpcAddOns<Node>
{
    let node_config = NodeConfig::load_from_config(Some(config.node_config)).unwrap();
    let node_address = signer.address();

    // NOTE:
    // no key is installed and this is strictly for internal usage. Realsically, we
    // should build a alloy provider impl that just uses the raw underlying db
    // so it will be quicker than rpc + won't be bounded by the rpc threadpool.

    let querying_provider: Arc<_> = ProviderBuilder::<_, _, Ethereum>::default()
        .with_recommended_fillers()
        .on_builtin(node.rpc_server_handle().http_url().unwrap().as_str())
        .await
        .unwrap()
        .into();

    let mev_boost_provider =
        MevBoostProvider::new_from_urls(querying_provider.clone(), &config.mev_boost_endpoints);

    tracing::info!(target: "angstrom::startup-sequence", "waiting for the next block to continue startup sequence. \
        this is done to ensure all modules start on the same state and we don't hit the rare  \
        condition of a block while starting modules");

    let _ = node
        .provider
        .subscribe_to_canonical_state()
        .recv()
        .await
        .expect("startup sequence failed");

    tracing::info!(target: "angstrom::startup-sequence", "new block detected. initializing all modules");

    let block_id = querying_provider.get_block_number().await.unwrap();

    let global_block_sync = GlobalBlockSync::new(block_id);

    let pool_config_store = Arc::new(
        AngstromPoolConfigStore::load_from_chain(
            node_config.angstrom_address,
            BlockId::Number(BlockNumberOrTag::Number(block_id)),
            &querying_provider
        )
        .await
        .unwrap()
    );

    let uniswap_registry: UniswapPoolRegistry = node_config.pools.into();
    let uni_ang_registry =
        UniswapAngstromRegistry::new(uniswap_registry.clone(), pool_config_store.clone());

    let periphery_c = ControllerV1::new(node_config.periphery_addr, querying_provider.clone());
    let node_set = periphery_c
        .nodes()
        .call()
        .await
        .unwrap()
        ._0
        .into_iter()
        .collect::<HashSet<_>>();

    // Build our PoolManager using the PoolConfig and OrderStorage we've already
    // created
    let eth_handle = EthDataCleanser::spawn(
        node_config.angstrom_address,
        node_config.periphery_addr,
        node.provider.subscribe_to_canonical_state(),
        executor.clone(),
        handles.eth_tx,
        handles.eth_rx,
        HashSet::new(),
        pool_config_store.clone(),
        global_block_sync.clone(),
        node_set,
        vec![handles.eth_handle_tx.take().unwrap()]
    )
    .unwrap();

    let uniswap_pool_manager = configure_uniswap_manager(
        querying_provider.clone(),
        eth_handle.subscribe_cannon_state_notifications().await,
        uniswap_registry,
        block_id,
        global_block_sync.clone(),
        node_config.pool_manager_address
    )
    .await;

    let uniswap_pools = uniswap_pool_manager.pools();
    executor.spawn(Box::pin(uniswap_pool_manager));
    let price_generator =
        TokenPriceGenerator::new(querying_provider.clone(), block_id, uniswap_pools.clone(), None)
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

    let network_handle = network_builder
        .with_pool_manager(handles.pool_tx)
        .with_consensus_manager(handles.consensus_tx_op)
        .build_handle(executor.clone(), node.provider.clone());

    let pool_config = PoolConfig::default();
    let order_storage = Arc::new(OrderStorage::new(&pool_config));
    let angstrom_pool_tracker =
        AngstromPoolsTracker::new(node_config.angstrom_address, pool_config_store.clone());

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
        angstrom_pool_tracker,
        handles.pool_manager_tx
    );

    // TODO load the stakes from Eigen using node.provider
    let validators = vec![
        AngstromValidator::new(PeerId::default(), 100),
        AngstromValidator::new(PeerId::default(), 200),
        AngstromValidator::new(PeerId::default(), 300),
    ];

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

    let _consensus_handle = executor.spawn_critical("consensus", Box::pin(manager));
    // ensure no more modules can be added to block sync.
    global_block_sync.finalize_modules();
}
