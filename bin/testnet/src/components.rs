use std::{collections::HashSet, sync::Arc};

use alloy::{
    eips::{BlockId, BlockNumberOrTag},
    network::EthereumWallet,
    providers::{network::Ethereum, Provider, ProviderBuilder},
    signers::{k256::ecdsa::SigningKey, local::LocalSigner}
};
use alloy_chains::Chain;
use alloy_primitives::Address;
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
    primitive::{PeerId, UniswapPoolRegistry},
    reth_db_wrapper::RethDbWrapper
};
use consensus::{AngstromValidator, ConsensusManager, ManagerNetworkDeps, Signer};
use matching_engine::{self, configure_uniswap_manager};
use order_pool::{order_storage::OrderStorage, PoolConfig, PoolManagerUpdate};
use reth::{
    api::NodeAddOns,
    builder::FullNodeComponents,
    providers::{BlockNumReader, CanonStateSubscriptions},
    tasks::TaskExecutor
};
use reth_metrics::common::mpsc::{UnboundedMeteredReceiver, UnboundedMeteredSender};
use reth_network_peers::pk2id;
use reth_node_builder::FullNode;
use secp256k1::{PublicKey, Secp256k1, SecretKey};
use tokio::sync::mpsc::{
    channel, unbounded_channel, Receiver, Sender, UnboundedReceiver, UnboundedSender
};
use validation::{
    init_validation,
    order::state::pools::AngstromPoolsTracker,
    validator::{ValidationClient, ValidationRequest}
};

use crate::config::{AngstromDevnetCli, FullTestnetNodeConfig};

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
    pub consensus_rx_op: UnboundedMeteredReceiver<StromConsensusEvent>
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
        consensus_rx_op
    }
}

// pub async fn initialize_strom_components<Node: FullNodeComponents, AddOns:
// NodeAddOns<Node>>(     cli: AngstromDevnetCli,
//     secret_key: SecretKey,
//     handles: StromHandles,
//     network_builder: StromNetworkBuilder,
//     node: FullNode<Node, AddOns>,
//     executor: &TaskExecutor
// ) {
//     let angstrom_address = Address::default();
//     let node_config =
// FullTestnetNodeConfig::load_from_config(Some(cli.node_config)).unwrap();

//     // I am sure there is a prettier way of doing this
//     let provider: Arc<_> = ProviderBuilder::<_, _, Ethereum>::default()
//         .with_recommended_fillers()
//         .wallet(EthereumWallet::from(
//
// LocalSigner::<SigningKey>::from_bytes(&secret_key.secret_bytes().into()).
// unwrap()         ))
//         .on_builtin(node.rpc_server_handles.rpc.http_url().unwrap().as_str())
//         .await
//         .unwrap()
//         .into();

//     let block_id = provider.get_block_number().await.unwrap();
//     let pool_config_store = Arc::new(
//         AngstromPoolConfigStore::load_from_chain(
//             angstrom_address,
//             BlockId::Number(BlockNumberOrTag::Number(block_id)),
//             &provider
//         )
//         .await
//         .unwrap()
//     );

//     let uniswap_registry: UniswapPoolRegistry = node_config.pools.into();
//     let uni_ang_registry =
//         UniswapAngstromRegistry::new(uniswap_registry.clone(),
// pool_config_store.clone());     let uniswap_pool_manager =
// configure_uniswap_manager(         provider.clone(),
//         node.provider.subscribe_to_canonical_state(),
//         uniswap_registry,
//         block_id
//     )
//     .await;
//     let uniswap_pools = uniswap_pool_manager.pools();
//     executor.spawn(Box::pin(async move {
//         uniswap_pool_manager
//             .watch_state_changes()
//             .await
//             .expect("watch for uniswap pool changes");
//     }));

//     let price_generator =
//         TokenPriceGenerator::new(provider.clone(), block_id,
// uniswap_pools.clone())             .await
//             .expect("failed to start token price generator");

//     let block_height = node.provider.best_block_number().unwrap();
//     init_validation(
//         RethDbWrapper::new(node.provider.clone()),
//         block_height,
//         Some(angstrom_address),
//         node.provider.canonical_state_stream(),
//         uniswap_pools.clone(),
//         price_generator,
//         pool_config_store.clone(),
//         handles.validator_rx
//     );

//     let network_handle = network_builder
//         .with_pool_manager(handles.pool_tx)
//         .with_consensus_manager(handles.consensus_tx_op)
//         .build_handle(executor.clone(), node.provider.clone());

//     let pool_config = PoolConfig::default();
//     let order_storage = Arc::new(OrderStorage::new(&pool_config));
//     let angstrom_pool_tracker =
//         AngstromPoolsTracker::new(angstrom_address,
// pool_config_store.clone());

//     // Build our PoolManager using the PoolConfig and OrderStorage we've
// already     // created
//     let eth_handle = EthDataCleanser::spawn(
//         angstrom_address,
//         node.provider.subscribe_to_canonical_state(),
//         executor.clone(),
//         handles.eth_tx,
//         handles.eth_rx,
//         HashSet::new(),
//         pool_config_store.clone()
//     )
//     .unwrap();

//     let _pool_handle = PoolManagerBuilder::new(
//         ValidationClient(handles.validator_tx.clone()),
//         Some(order_storage.clone()),
//         network_handle.clone(),
//         eth_handle.subscribe_network(),
//         handles.pool_rx
//     )
//     .with_config(pool_config)
//     .build_with_channels(
//         executor.clone(),
//         handles.orderpool_tx,
//         handles.orderpool_rx,
//         angstrom_pool_tracker,
//         handles.pool_manager_tx
//     );

//     let signer = Signer::new(secret_key);

//     // TODO load the stakes from Eigen using node.provider
//     let validators = vec![
//         AngstromValidator::new(PeerId::default(), 100),
//         AngstromValidator::new(PeerId::default(), 200),
//         AngstromValidator::new(PeerId::default(), 300),
//     ];

//     let manager = ConsensusManager::new(
//         ManagerNetworkDeps::new(
//             network_handle.clone(),
//             node.provider.subscribe_to_canonical_state(),
//             handles.consensus_rx_op
//         ),
//         signer,
//         validators,
//         order_storage.clone(),
//         block_height,
//         uni_ang_registry,
//         uniswap_pools.clone(),
//         provider
//     );
//     let _consensus_handle = executor.spawn_critical("consensus",
// Box::pin(manager)); }
