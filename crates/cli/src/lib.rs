pub mod angstrom;
mod components;
mod config;
mod metrics;

use std::{collections::HashSet, sync::Arc};

use alloy::primitives::Address;
use angstrom_amm_quoter::QuoterHandle;
use angstrom_network::{AngstromNetworkBuilder, pool_manager::PoolHandle};
use angstrom_rpc::{
    ConsensusApi, OrderApi,
    api::{ConsensusApiServer, OrderApiServer}
};
use angstrom_types::primitive::{AngstromMetaSigner, AngstromSigner};
use consensus::ConsensusHandler;
use parking_lot::RwLock;
use reth::{chainspec::ChainSpec, tasks::TaskExecutor};
use reth_db::DatabaseEnv;
use reth_node_builder::{Node, NodeBuilder, NodeHandle, WithLaunchContext};
use reth_node_ethereum::{EthereumAddOns, EthereumNode};
use validation::validator::ValidationClient;

use crate::{
    components::{StromHandles, init_network_builder, initialize_strom_components},
    config::AngstromConfig
};

pub(crate) async fn run_with_signer<S: AngstromMetaSigner>(
    pool: PoolHandle,
    executor: TaskExecutor,
    node_set: HashSet<Address>,
    validation_client: ValidationClient,
    quoter_handle: QuoterHandle,
    consensus_client: ConsensusHandler,
    secret_key: AngstromSigner<S>,
    args: AngstromConfig,
    mut channels: StromHandles,
    builder: WithLaunchContext<NodeBuilder<Arc<DatabaseEnv>, ChainSpec>>
) -> eyre::Result<()> {
    let mut network = init_network_builder(
        secret_key.clone(),
        channels.eth_handle_rx.take().unwrap(),
        Arc::new(RwLock::new(node_set.clone()))
    )?;

    let protocol_handle = network.build_protocol_handler();
    let cloned_consensus_client = consensus_client.clone();
    let executor_clone = executor.clone();
    let NodeHandle { node, node_exit_future } = builder
        .with_types::<EthereumNode>()
        .with_components(
            EthereumNode::default()
                .components_builder()
                .network(AngstromNetworkBuilder::new(protocol_handle))
        )
        .with_add_ons::<EthereumAddOns<_, _, _>>(Default::default())
        .extend_rpc_modules(move |rpc_context| {
            let order_api = OrderApi::new(
                pool.clone(),
                executor_clone.clone(),
                validation_client,
                quoter_handle
            );
            let consensus = ConsensusApi::new(cloned_consensus_client, executor_clone);
            rpc_context.modules.merge_configured(order_api.into_rpc())?;
            rpc_context.modules.merge_configured(consensus.into_rpc())?;

            Ok(())
        })
        .launch()
        .await?;
    network = network.with_reth(node.network.clone());

    initialize_strom_components(
        args,
        secret_key,
        channels,
        network,
        &node,
        executor,
        node_exit_future,
        node_set,
        consensus_client
    )
    .await
}
