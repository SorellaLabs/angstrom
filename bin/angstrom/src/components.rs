//! CLI definition and entrypoint to executable

use std::{collections::HashSet, sync::Arc};

use alloy::{
    eips::{BlockId, BlockNumberOrTag},
    network::{EthereumWallet, Network},
    providers::{network::Ethereum, Provider, ProviderBuilder},
    signers::{k256::ecdsa::SigningKey, local::LocalSigner},
    transports::Transport
};
use alloy_chains::Chain;
use alloy_primitives::{Address, BlockNumber};
use angstrom_eth::{
    handle::{Eth, EthCommand},
    manager::EthDataCleanser
};
use angstrom_network::{
    manager::StromConsensusEvent,
    pool_manager::{OrderCommand, PoolHandle},
    NetworkBuilder as StromNetworkBuilder, NetworkOrderEvent, PoolManagerBuilder, StatusState,
    VerificationSidecar
};
use angstrom_types::{
    contract_payloads::angstrom::{AngstromPoolConfigStore, UniswapAngstromRegistry},
    primitive::{PeerId, PoolId as AngstromPoolId, UniswapPoolRegistry},
    reth_db_wrapper::RethDbWrapper
};
use consensus::{AngstromValidator, ConsensusManager, ManagerNetworkDeps, Signer};
use matching_engine::{manager::MatcherCommand, MatchingManager};
use order_pool::{order_storage::OrderStorage, PoolConfig, PoolManagerUpdate};
use reth::{
    api::NodeAddOns,
    builder::FullNodeComponents,
    providers::{BlockNumReader, CanonStateNotifications, CanonStateSubscriptions},
    tasks::TaskExecutor
};
use reth_metrics::common::mpsc::{UnboundedMeteredReceiver, UnboundedMeteredSender};
use reth_network_peers::pk2id;
use reth_node_builder::FullNode;
use secp256k1::{PublicKey, Secp256k1, SecretKey};
use tokio::sync::mpsc::{
    channel, unbounded_channel, Receiver, Sender, UnboundedReceiver, UnboundedSender
};
use uniswap_v4::uniswap::{
    pool::EnhancedUniswapPool, pool_data_loader::DataLoader, pool_manager::UniswapPoolManager,
    pool_providers::canonical_state_adapter::CanonicalStateAdapter
};
use validation::{
    common::TokenPriceGenerator,
    init_validation,
    order::state::pools::AngstromPoolsTracker,
    validator::{ValidationClient, ValidationRequest}
};

pub fn init_network_builder(secret_key: SecretKey) -> eyre::Result<StromNetworkBuilder> {
    let public_key = PublicKey::from_secret_key(&Secp256k1::new(), &secret_key);

    let state = StatusState {
        version:   0,
        chain:     Chain::mainnet().id(),
        peer:      pk2id(&public_key),
        timestamp: 0
    };

    let verification =
        VerificationSidecar { status: state, has_sent: false, has_received: false, secret_key };

    Ok(StromNetworkBuilder::new(verification))
}
use crate::{cli::NodeConfig, AngstromConfig};
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

    pub pool_manager_tx: tokio::sync::broadcast::Sender<PoolManagerUpdate>,

    pub consensus_tx_op: UnboundedMeteredSender<StromConsensusEvent>,
    pub consensus_rx_op: UnboundedMeteredReceiver<StromConsensusEvent>,

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
        matching_rx
    }
}

pub async fn initialize_strom_components<Node: FullNodeComponents, AddOns: NodeAddOns<Node>>(
    angstrom_address: Option<Address>,
    config: AngstromConfig,
    secret_key: SecretKey,
    handles: StromHandles,
    network_builder: StromNetworkBuilder,
    node: FullNode<Node, AddOns>,
    executor: &TaskExecutor
) {
    let node_config = NodeConfig::load_from_config(Some(config.node_config)).unwrap();

    let signer = LocalSigner::<SigningKey>::from_bytes(&secret_key.secret_bytes().into()).unwrap();
    let node_address = signer.address();

    // I am sure there is a prettier way of doing this
    let provider: Arc<_> = ProviderBuilder::<_, _, Ethereum>::default()
        .with_recommended_fillers()
        .wallet(EthereumWallet::from(signer))
        .on_builtin(node.rpc_server_handles.rpc.http_url().unwrap().as_str())
        .await
        .unwrap()
        .into();

    let block_id = provider.get_block_number().await.unwrap();
    let pool_config_store = Arc::new(
        AngstromPoolConfigStore::load_from_chain(
            node_config.angstrom_address,
            BlockId::Number(BlockNumberOrTag::Number(block_id)),
            &provider
        )
        .await
        .unwrap()
    );

    let uniswap_registry: UniswapPoolRegistry = node_config.pools.into();
    let uni_ang_registry =
        UniswapAngstromRegistry::new(uniswap_registry.clone(), pool_config_store.clone());
    let uniswap_pool_manager = configure_uniswap_manager(
        provider.clone(),
        node.provider.subscribe_to_canonical_state(),
        uniswap_registry,
        block_id
    )
    .await;
    let uniswap_pools = uniswap_pool_manager.pools();
    executor.spawn(Box::pin(async move {
        uniswap_pool_manager
            .watch_state_changes()
            .await
            .expect("watch for uniswap pool changes");
    }));

    let price_generator =
        TokenPriceGenerator::new(provider.clone(), block_id, uniswap_pools.clone())
            .await
            .expect("failed to start token price generator");

    let block_height = node.provider.best_block_number().unwrap();

    init_validation(
        RethDbWrapper::new(node.provider.clone()),
        block_height,
        angstrom_address,
        node_address,
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

    // Build our PoolManager using the PoolConfig and OrderStorage we've already
    // created
    let eth_handle = EthDataCleanser::spawn(
        angstrom_address.unwrap_or(node_config.angstrom_address),
        node.provider.subscribe_to_canonical_state(),
        executor.clone(),
        handles.eth_tx,
        handles.eth_rx,
        HashSet::new(),
        pool_config_store.clone()
    )
    .unwrap();

    let _pool_handle = PoolManagerBuilder::new(
        validation_handle.clone(),
        Some(order_storage.clone()),
        network_handle.clone(),
        eth_handle.subscribe_network(),
        handles.pool_rx
    )
    .with_config(pool_config)
    .build_with_channels(
        executor.clone(),
        handles.orderpool_tx,
        handles.orderpool_rx,
        angstrom_pool_tracker,
        handles.pool_manager_tx
    );

    let signer = Signer::new(secret_key);

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
            node.provider.subscribe_to_canonical_state(),
            handles.consensus_rx_op
        ),
        signer,
        validators,
        order_storage.clone(),
        block_height,
        uni_ang_registry,
        uniswap_pools.clone(),
        provider,
        matching_handle
    );

    let _consensus_handle = executor.spawn_critical("consensus", Box::pin(manager));
}

async fn configure_uniswap_manager<T: Transport + Clone, N: Network>(
    provider: Arc<impl Provider<T, N>>,
    state_notification: CanonStateNotifications,
    uniswap_pool_registry: UniswapPoolRegistry,
    current_block: BlockNumber
) -> UniswapPoolManager<CanonicalStateAdapter, DataLoader<AngstromPoolId>, AngstromPoolId> {
    let mut uniswap_pools: Vec<_> = uniswap_pool_registry
        .pools()
        .keys()
        .map(|pool_id| {
            let initial_ticks_per_side = 200;
            EnhancedUniswapPool::new(
                DataLoader::new_with_registry(*pool_id, uniswap_pool_registry.clone()),
                initial_ticks_per_side
            )
        })
        .collect();

    for pool in uniswap_pools.iter_mut() {
        pool.initialize(Some(current_block), provider.clone())
            .await
            .unwrap();
    }

    let state_change_buffer = 100;
    UniswapPoolManager::new(
        uniswap_pools,
        current_block,
        state_change_buffer,
        Arc::new(CanonicalStateAdapter::new(state_notification))
    )
}
