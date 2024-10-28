//! CLI definition and entrypoint to executable
use std::{collections::HashSet, path::PathBuf, sync::Arc};

use alloy::network::EthereumWallet;
use angstrom_metrics::{initialize_prometheus_metrics, METRICS_ENABLED};
use angstrom_network::manager::StromConsensusEvent;
use angstrom_types::reth_db_wrapper::RethDbWrapper;
use order_pool::{order_storage::OrderStorage, PoolConfig, PoolManagerUpdate};
use reth_node_builder::{FullNode, NodeHandle};
use secp256k1::{PublicKey, Secp256k1, SecretKey};
use tokio::sync::mpsc::{
    channel, unbounded_channel, Receiver, Sender, UnboundedReceiver, UnboundedSender
};

mod network_builder;
use alloy::{
    eips::{BlockId, BlockNumberOrTag},
    providers::{network::Ethereum, Provider, ProviderBuilder},
    signers::{k256::ecdsa::SigningKey, local::LocalSigner}
};
use alloy_chains::Chain;
use alloy_primitives::{private::serde::Deserialize, Address};
use angstrom_eth::{
    handle::{Eth, EthCommand},
    manager::EthDataCleanser
};
use angstrom_network::{
    pool_manager::{OrderCommand, PoolHandle},
    NetworkBuilder as StromNetworkBuilder, NetworkOrderEvent, PoolManagerBuilder, StatusState,
    VerificationSidecar
};
use angstrom_rpc::{api::OrderApiServer, OrderApi};
use angstrom_types::{
    contract_payloads::angstrom::{AngstromPoolConfigStore, UniswapAngstromRegistry},
    primitive::{PeerId, PoolKey}
};
use clap::Parser;
use consensus::{AngstromValidator, ConsensusManager, ManagerNetworkDeps, Signer};
use eyre::Context;
use reth::{
    api::NodeAddOns,
    builder::{FullNodeComponents, Node},
    chainspec::EthereumChainSpecParser,
    cli::Cli,
    providers::{BlockNumReader, CanonStateSubscriptions},
    tasks::TaskExecutor
};
use reth_cli_util::get_secret_key;
use reth_metrics::common::mpsc::{UnboundedMeteredReceiver, UnboundedMeteredSender};
use reth_network_peers::pk2id;
use reth_node_ethereum::{node::EthereumAddOns, EthereumNode};
use validation::init_validation;

use crate::cli::network_builder::AngstromNetworkBuilder;

/// Convenience function for parsing CLI options, set up logging and run the
/// chosen command.
#[inline]
pub fn run() -> eyre::Result<()> {
    Cli::<EthereumChainSpecParser, AngstromConfig>::parse().run(|builder, args| async move {
        let executor = builder.task_executor().clone();

        if args.metrics {
            executor.spawn_critical("metrics", init_metrics(args.metrics_port));
            METRICS_ENABLED.set(true).unwrap();
        } else {
            METRICS_ENABLED.set(false).unwrap();
        }

        let secret_key = get_secret_key(&args.secret_key_location)?;

        let mut network = init_network_builder(secret_key)?;
        let protocol_handle = network.build_protocol_handler();
        let channels = initialize_strom_handles();

        // for rpc
        let pool = channels.get_pool_handle();
        let executor_clone = executor.clone();
        // let consensus = channels.get_consensus_handle();
        let NodeHandle { node, node_exit_future } = builder
            .with_types::<EthereumNode>()
            .with_components(
                EthereumNode::default()
                    .components_builder()
                    .network(AngstromNetworkBuilder::new(protocol_handle))
            )
            .with_add_ons::<EthereumAddOns>(Default::default())
            .extend_rpc_modules(move |rpc_context| {
                let order_api = OrderApi::new(pool.clone(), executor_clone);
                // let quotes_api = QuotesApi { pool: pool.clone() };
                // let consensus_api = ConsensusApi { consensus: consensus.clone() };
                rpc_context.modules.merge_configured(order_api.into_rpc())?;
                // rpc_context
                //     .modules
                //     .merge_configured(quotes_api.into_rpc())?;
                // rpc_context
                //     .modules
                //     .merge_configured(consensus_api.into_rpc())?;

                Ok(())
            })
            .launch()
            .await?;

        initialize_strom_components(args, secret_key, channels, network, node, &executor).await;

        node_exit_future.await
    })
}

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

    // pub fn get_consensus_handle(&self) -> ConsensusHandle {
    //     ConsensusHandle { sender: self.consensus_tx.clone() }
    // }
}

pub fn initialize_strom_handles() -> StromHandles {
    let (eth_tx, eth_rx) = channel(100);
    let (pool_manager_tx, _) = tokio::sync::broadcast::channel(100);
    let (pool_tx, pool_rx) = reth_metrics::common::mpsc::metered_unbounded_channel("orderpool");
    let (orderpool_tx, orderpool_rx) = unbounded_channel();
    let (consensus_tx_op, consensus_rx_op) =
        reth_metrics::common::mpsc::metered_unbounded_channel("orderpool");

    StromHandles {
        eth_tx,
        eth_rx,
        pool_tx,
        pool_rx,
        orderpool_tx,
        pool_manager_tx,
        orderpool_rx,
        consensus_tx_op,
        consensus_rx_op
    }
}

pub async fn initialize_strom_components<Node: FullNodeComponents, AddOns: NodeAddOns<Node>>(
    config: AngstromConfig,
    secret_key: SecretKey,
    handles: StromHandles,
    network_builder: StromNetworkBuilder,
    node: FullNode<Node, AddOns>,
    executor: &TaskExecutor
) {
    let node_config = NodeConfig::load_from_config(Some(config.node_config)).unwrap();
    let eth_handle = EthDataCleanser::spawn(
        node_config.angstrom_address,
        node.provider.subscribe_to_canonical_state(),
        node.provider.clone(),
        executor.clone(),
        handles.eth_tx,
        handles.eth_rx,
        HashSet::new()
    )
    .unwrap();

    let network_handle = network_builder
        .with_pool_manager(handles.pool_tx)
        .with_consensus_manager(handles.consensus_tx_op)
        .build_handle(executor.clone(), node.provider.clone());
    let block_height = node.provider.best_block_number().unwrap();
    let validator = init_validation(
        RethDbWrapper::new(node.provider.clone()),
        config.validation_cache_size,
        block_height,
        node.provider.subscribe_to_canonical_state()
    );

    // Create our pool config
    let pool_config = PoolConfig::default();

    // Create order storage based on that config
    let order_storage = Arc::new(OrderStorage::new(&pool_config));

    // Build our PoolManager using the PoolConfig and OrderStorage we've already
    // created
    let _pool_handle = PoolManagerBuilder::new(
        validator.clone(),
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
        handles.pool_manager_tx
    );

    let signer = Signer::new(secret_key);

    // TODO load the stakes from Eigen using node.provider
    // list of PeerIds will be known upfront on the first version
    let validators = vec![
        AngstromValidator::new(PeerId::default(), 100),
        AngstromValidator::new(PeerId::default(), 200),
        AngstromValidator::new(PeerId::default(), 300),
    ];

    // I am sure there is a prettier way of doing this
    let provider = ProviderBuilder::<_, _, Ethereum>::default()
        .with_recommended_fillers()
        .wallet(EthereumWallet::from(
            LocalSigner::<SigningKey>::from_bytes(&secret_key.secret_bytes().into()).unwrap()
        ))
        .on_builtin(node.rpc_server_handles.rpc.http_url().unwrap().as_str())
        .await
        .unwrap();
    let block_id = provider.get_block_number().await.unwrap();
    let pool_config_store = AngstromPoolConfigStore::load_from_chain(
        node_config.angstrom_address,
        BlockId::Number(BlockNumberOrTag::Number(block_id)),
        &provider
    )
    .await
    .unwrap();
    let pool_registry = UniswapAngstromRegistry::new(node_config.pools.into(), pool_config_store);
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
        pool_registry,
        provider
    );
    let _consensus_handle = executor.spawn_critical("consensus", Box::pin(manager));
}

#[derive(Debug, Clone, Default, clap::Args)]
pub struct AngstromConfig {
    #[clap(long)]
    pub mev_guard:             bool,
    #[clap(long)]
    pub secret_key_location:   PathBuf,
    #[clap(long)]
    pub node_config:           PathBuf,
    // default is 100mb
    #[clap(long, default_value = "1000000")]
    pub validation_cache_size: usize,
    /// enables the metrics
    #[clap(long, default_value = "false", global = true)]
    pub metrics:               bool,
    /// spawns the prometheus metrics exporter at the specified port
    /// Default: 6969
    #[clap(long, default_value = "6969", global = true)]
    pub metrics_port:          u16
}

#[derive(Debug, Clone, Deserialize)]
pub struct NodeConfig {
    pub secret_key:       String,
    pub angstrom_address: Address,
    pub pools:            Vec<PoolKey>
}

impl NodeConfig {
    pub fn load_from_config(config: Option<PathBuf>) -> Result<Self, eyre::Report> {
        let config_path = config.ok_or_else(|| eyre::eyre!("Config path not provided"))?;

        if !config_path.exists() {
            return Err(eyre::eyre!("Config file does not exist at {:?}", config_path));
        }

        let toml_content = std::fs::read_to_string(&config_path)
            .wrap_err_with(|| format!("Could not read config file {:?}", config_path))?;

        let node_config: NodeConfig = toml::from_str(&toml_content)
            .wrap_err_with(|| format!("Could not deserialize config file {:?}", config_path))?;

        Ok(node_config)
    }
}

async fn init_metrics(metrics_port: u16) {
    let _ = initialize_prometheus_metrics(metrics_port)
        .await
        .inspect_err(|e| eprintln!("failed to start metrics endpoint - {:?}", e));
}
