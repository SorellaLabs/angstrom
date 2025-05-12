use std::{
    collections::{HashMap, HashSet},
    pin::Pin,
    sync::Arc,
    time::Duration
};

use alloy::{
    self,
    eips::{BlockId, BlockNumberOrTag},
    network::Ethereum,
    primitives::Address,
    providers::{Provider, ProviderBuilder}
};
use alloy_chains::Chain;
use angstrom::{
    cli::{AngstromConfig, NodeConfig},
    components::{StromHandles, initialize_strom_handles}
};
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
    BlockReader, CanonStateNotifications, DatabaseProviderFactory, ReceiptProvider,
    TryIntoHistoricalStateProvider
};
use telemetry::init_telemetry;
use tokio::sync::mpsc::{
    Receiver, Sender, UnboundedReceiver, UnboundedSender, channel, unbounded_channel
};
use uniswap_v4::{configure_uniswap_manager, fetch_angstrom_pools};
use validation::{
    common::TokenPriceGenerator,
    init_validation,
    validator::{ValidationClient, ValidationRequest}
};

pub async fn initialize_strom_components_at_block<Node, AddOns, P: Peers + Unpin + 'static>(
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
    Ok(())
}
