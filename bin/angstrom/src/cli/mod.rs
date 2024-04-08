//! CLI definition and entrypoint to executable
use std::path::{Path, PathBuf};

use reth_node_builder::{FullNode, NodeHandle};
use secp256k1::{PublicKey, Secp256k1};
use tokio::sync::mpsc::{
    channel, unbounded_channel, Receiver, Sender, UnboundedReceiver, UnboundedSender
};
mod network_builder;

use angstrom_eth::{
    handle::{self, Eth, EthCommand, EthHandle},
    manager::EthDataCleanser
};
use angstrom_network::{
    pool_manager::{OrderCommand, PoolHandle},
    NetworkBuilder as StromNetworkBuilder, NetworkOrderEvent, PoolManagerBuilder, StatusState,
    StromNetworkHandle, StromProtocolHandler, VerificationSidecar
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
use consensus::{ConsensusCommand, ConsensusHandle, ConsensusManager};
use reth::{
    args::get_secret_key,
    builder::{
        components::{FullNodeComponents, NetworkBuilder},
        Node
    },
    cli::{
        config::{RethNetworkConfig, RethRpcConfig},
        Cli
    },
    dirs::{DataDirPath, MaybePlatformPath},
    primitives::{Chain, PeerId},
    providers::CanonStateSubscriptions,
    rpc::types::pk_to_id,
    tasks::TaskExecutor
};
use reth_metrics::common::mpsc::{UnboundedMeteredReceiver, UnboundedMeteredSender};
use reth_network::NetworkConfigBuilder;
use reth_node_ethereum::EthereumNode;
use validation::{
    init_validation,
    validator::{ValidationClient, ValidationRequest}
};

use self::network_builder::AngstromNetworkBuilder;

/// Convenience function for parsing CLI options, set up logging and run the
/// chosen command.
#[inline]
pub fn run() -> eyre::Result<()> {
    Cli::<AngstromConfig>::parse().run(|builder, args| async move {
        let executor = builder.task_executor().clone();

        let mut network = init_network_builder(&args, &executor)?;
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

pub fn init_network_builder(
    config: &AngstromConfig,
    executor: &TaskExecutor
) -> eyre::Result<StromNetworkBuilder> {
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

type DefaultPoolHandle = PoolHandle<
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
    eth_tx: Sender<EthCommand>,
    eth_rx: Receiver<EthCommand>,

    validation_tx: UnboundedSender<ValidationRequest>,
    validation_rx: UnboundedReceiver<ValidationRequest>,

    pool_tx: UnboundedMeteredSender<NetworkOrderEvent>,
    pool_rx: UnboundedMeteredReceiver<NetworkOrderEvent>,

    orderpool_tx: UnboundedSender<DefaultOrderCommand>,
    orderpool_rx: UnboundedReceiver<DefaultOrderCommand>,

    consensus_tx: Sender<ConsensusCommand>,
    consensus_rx: Receiver<ConsensusCommand>
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
    let (validation_tx, validation_rx) = unbounded_channel();
    let (orderpool_tx, orderpool_rx) = unbounded_channel();

    StromHandles {
        eth_tx,
        eth_rx,
        validation_tx,
        validation_rx,
        pool_tx,
        pool_rx,
        orderpool_tx,
        orderpool_rx,
        consensus_tx,
        consensus_rx
    }
}

pub fn initialize_strom_components<Node: FullNodeComponents>(
    config: AngstromConfig,
    handles: StromHandles,
    network_builder: StromNetworkBuilder,
    node: FullNode<Node>,
    executor: &TaskExecutor
) {
    let (consensus_tx, consensus_rx) =
        reth_metrics::common::mpsc::metered_unbounded_channel("orderpool");

    let eth_handle = EthDataCleanser::new(
        node.provider.subscribe_to_canonical_state(),
        node.provider.clone(),
        executor.clone(),
        handles.eth_tx,
        handles.eth_rx
    )
    .unwrap();

    let network_handle = network_builder
        .with_pool_manager(handles.pool_tx)
        .with_consensus_manager(consensus_tx)
        .build(executor.clone(), node.provider.clone());

    let validator = init_validation(node.provider.clone(), eth_handle.subscribe_network_stream());

    let pool_handle = PoolManagerBuilder::new(
        validator.clone(),
        network_handle.clone(),
        eth_handle.subscribe_network(),
        handles.pool_rx
    )
    .build_with_channels(executor.clone(), handles.orderpool_tx, handles.orderpool_rx);

    let consensus_handle = ConsensusManager::new(
        executor.clone(),
        network_handle.clone(),
        pool_handle.clone(),
        validator.clone(),
        node.provider.subscribe_to_canonical_state(),
        consensus_rx,
        handles.consensus_tx,
        handles.consensus_rx
    );
}

#[derive(Debug, Clone, Default, clap::Args)]
struct AngstromConfig {
    #[clap(long)]
    pub mev_guard: bool,

    #[clap(long)]
    pub secret_key_location: PathBuf
}
