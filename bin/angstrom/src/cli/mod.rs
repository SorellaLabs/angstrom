//! CLI definition and entrypoint to executable
use std::path::PathBuf;

use angstrom_network::manager::StromConsensusEvent;
use reth_node_builder::{FullNode, NodeHandle};
use secp256k1::{PublicKey, Secp256k1};
use tokio::sync::mpsc::{
    channel, unbounded_channel, Receiver, Sender, UnboundedReceiver, UnboundedSender
};
mod network_builder;

use angstrom_eth::{
    handle::{Eth, EthCommand},
    manager::EthDataCleanser
};
use angstrom_network::{
    pool_manager::{OrderCommand, PoolHandle},
    NetworkBuilder as StromNetworkBuilder, NetworkOrderEvent, PoolManagerBuilder, StatusState,
    VerificationSidecar
};
use angstrom_rpc::{
    api::{ConsensusApiServer, OrderApiServer, QuotingApiServer},
    ConsensusApi, OrderApi, QuotesApi
};
use angstrom_types::rpc::{
    EcRecoveredComposableLimitOrder, EcRecoveredComposableSearcherOrder, EcRecoveredLimitOrder,
    EcRecoveredSearcherOrder
};
use clap::Parser;
use consensus::{ConsensusCommand, ConsensusHandle, ConsensusManager, ManagerNetworkDeps, Signer};
use reth::{
    args::get_secret_key,
    builder::{components::FullNodeComponents, Node},
    cli::Cli,
    primitives::Chain,
    providers::CanonStateSubscriptions,
    rpc::types::pk_to_id,
    tasks::TaskExecutor
};
use reth_metrics::common::mpsc::{UnboundedMeteredReceiver, UnboundedMeteredSender};
use reth_node_ethereum::EthereumNode;
use validation::init_validation;

use self::network_builder::AngstromNetworkBuilder;

/// Convenience function for parsing CLI options, set up logging and run the
/// chosen command.
#[inline]
pub fn run() -> eyre::Result<()> {
    Cli::<AngstromConfig>::parse().run(|builder, args| async move {
        let executor = builder.task_executor().clone();

        let mut network = init_network_builder(&args)?;
        let protocol_handle = network.build_protocol_handler();
        let channels = initialize_strom_handles();

        // for rpc
        let pool = channels.get_pool_handle();
        let consensus = channels.get_consensus_handle();

        let NodeHandle { node, node_exit_future } = builder
            .with_types(EthereumNode::default())
            .with_components(
                EthereumNode::default()
                    .components()
                    .network(AngstromNetworkBuilder::new(protocol_handle))
            )
            .extend_rpc_modules(move |rpc_components| {
                let order_api = OrderApi { pool: pool.clone() };
                let quotes_api = QuotesApi { pool: pool.clone() };
                let consensus_api = ConsensusApi { consensus: consensus.clone() };

                rpc_components
                    .modules
                    .merge_configured(order_api.into_rpc())?;
                rpc_components
                    .modules
                    .merge_configured(quotes_api.into_rpc())?;
                rpc_components
                    .modules
                    .merge_configured(consensus_api.into_rpc())?;

                Ok(())
            })
            .launch()
            .await?;

        initialize_strom_components(args, channels, network, node, &executor);

        node_exit_future.await
    })
}

pub fn init_network_builder(config: &AngstromConfig) -> eyre::Result<StromNetworkBuilder> {
    let secret_key = get_secret_key(&config.secret_key_location)?;
    let public_key = PublicKey::from_secret_key(&Secp256k1::new(), &secret_key);

    let state = StatusState {
        version:   0,
        chain:     Chain::mainnet(),
        peer:      pk_to_id(&public_key),
        timestamp: 0
    };

    let verification =
        VerificationSidecar { status: state, has_sent: false, has_received: false, secret_key };

    Ok(StromNetworkBuilder::new(verification))
}

pub type DefaultPoolHandle = PoolHandle<
    EcRecoveredLimitOrder,
    EcRecoveredComposableLimitOrder,
    EcRecoveredSearcherOrder,
    EcRecoveredComposableSearcherOrder
>;

type DefaultOrderCommand = OrderCommand<
    EcRecoveredLimitOrder,
    EcRecoveredComposableLimitOrder,
    EcRecoveredSearcherOrder,
    EcRecoveredComposableSearcherOrder
>;

// due to how the init process works with reth. we need to init like this
pub struct StromHandles {
    pub eth_tx: Sender<EthCommand>,
    pub eth_rx: Receiver<EthCommand>,

    pub pool_tx: UnboundedMeteredSender<NetworkOrderEvent>,
    pub pool_rx: UnboundedMeteredReceiver<NetworkOrderEvent>,

    pub orderpool_tx: UnboundedSender<DefaultOrderCommand>,
    pub orderpool_rx: UnboundedReceiver<DefaultOrderCommand>,

    pub consensus_tx:    Sender<ConsensusCommand>,
    pub consensus_rx:    Receiver<ConsensusCommand>,
    pub consensus_tx_op: UnboundedMeteredSender<StromConsensusEvent>,
    pub consensus_rx_op: UnboundedMeteredReceiver<StromConsensusEvent>
}

impl StromHandles {
    pub fn get_pool_handle(&self) -> DefaultPoolHandle {
        PoolHandle { manager_tx: self.orderpool_tx.clone() }
    }

    pub fn get_consensus_handle(&self) -> ConsensusHandle {
        ConsensusHandle { sender: self.consensus_tx.clone() }
    }
}

pub fn initialize_strom_handles() -> StromHandles {
    let (eth_tx, eth_rx) = channel(100);
    let (consensus_tx, consensus_rx) = channel(100);
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
        orderpool_rx,
        consensus_tx,
        consensus_rx,
        consensus_tx_op,
        consensus_rx_op
    }
}

pub fn initialize_strom_components<Node: FullNodeComponents>(
    config: AngstromConfig,
    handles: StromHandles,
    network_builder: StromNetworkBuilder,
    node: FullNode<Node>,
    executor: &TaskExecutor
) {
    let eth_handle = EthDataCleanser::spawn(
        node.provider.subscribe_to_canonical_state(),
        node.provider.clone(),
        executor.clone(),
        handles.eth_tx,
        handles.eth_rx
    )
    .unwrap();

    let network_handle = network_builder
        .with_pool_manager(handles.pool_tx)
        .with_consensus_manager(handles.consensus_tx_op)
        .build_handle(executor.clone(), node.provider.clone());

    let validator = init_validation(
        node.provider.clone(),
        config.validation_cache_size,
        eth_handle.subscribe_network_stream()
    );

    let pool_handle = PoolManagerBuilder::new(
        validator.clone(),
        network_handle.clone(),
        eth_handle.subscribe_network(),
        handles.pool_rx
    )
    .build_with_channels(executor.clone(), handles.orderpool_tx, handles.orderpool_rx);

    let signer = Signer::default();

    let _consensus_handle = ConsensusManager::spawn(
        executor.clone(),
        ManagerNetworkDeps::new(
            network_handle.clone(),
            node.provider.subscribe_to_canonical_state(),
            handles.consensus_rx_op,
            handles.consensus_tx,
            handles.consensus_rx
        ),
        signer,
        pool_handle.clone(),
        validator.clone()
    );
}

#[derive(Debug, Clone, Default, clap::Args)]
pub struct AngstromConfig {
    #[clap(long)]
    pub mev_guard: bool,

    #[clap(long)]
    pub secret_key_location:   PathBuf,
    // default is 100mb
    #[clap(long, default_value = "1000000")]
    pub validation_cache_size: usize
}
