//! Angstrom binary executable.
use std::{collections::HashSet, sync::Arc};

use alloy::providers::{ProviderBuilder, network::Ethereum};
use alloy_chains::NamedChain;
use alloy_primitives::Address;
use angstrom_amm_quoter::QuoterHandle;
use angstrom_metrics::METRICS_ENABLED;
use angstrom_network::AngstromNetworkBuilder;
use angstrom_rpc::{
    ConsensusApi, OrderApi,
    api::{ConsensusApiServer, OrderApiServer}
};
use angstrom_types::{
    contract_bindings::controller_v_1::ControllerV1,
    primitive::{
        ANGSTROM_DOMAIN, AngstromMetaSigner, AngstromSigner, CONTROLLER_V1_ADDRESS,
        init_with_chain_id
    }
};
use clap::Parser;
use consensus::ConsensusHandler;
use parking_lot::RwLock;
use pool_manager::PoolHandle;
use reth::{
    chainspec::{ChainSpec, EthChainSpec, EthereumChainSpecParser},
    cli::Cli,
    tasks::TaskExecutor
};
use reth_db::DatabaseEnv;
use reth_node_builder::{Node, NodeBuilder, WithLaunchContext};
use reth_node_ethereum::{EthereumAddOns, EthereumNode};
use validation::validator::ValidationClient;

use crate::{
    components::{AngstromLauncher, init_network_builder},
    config::AngstromConfig,
    handles::{ConsensusHandles, ConsensusMode},
    metrics::init_metrics
};

/// Convenience function for parsing CLI options, set up logging and run the
/// chosen command.
#[inline]
pub fn run() -> eyre::Result<()> {
    Cli::<EthereumChainSpecParser, AngstromConfig>::parse().run(|builder, args| async move {
        let executor = builder.task_executor().clone();
        let chain = builder.config().chain.chain().named().unwrap();

        match chain {
            NamedChain::Sepolia => {
                init_with_chain_id(NamedChain::Sepolia as u64);
            }
            NamedChain::Mainnet => {
                init_with_chain_id(NamedChain::Mainnet as u64);
            }
            chain => panic!("we do not support chain {chain}")
        }

        if args.metrics_enabled {
            executor.spawn_critical("metrics", init_metrics(args.metrics_port));
            METRICS_ENABLED.set(true).unwrap();
        } else {
            METRICS_ENABLED.set(false).unwrap();
        }

        tracing::info!(domain=?ANGSTROM_DOMAIN);

        let channels = ConsensusHandles::new();
        let quoter_handle = QuoterHandle(channels.quoter_tx.clone());

        // for rpc
        let pool = channels.get_pool_handle();
        let executor_clone = executor.clone();
        let validation_client = ValidationClient(channels.validator_tx.clone());
        let consensus_client = ConsensusHandler(channels.mode.consensus_tx_rpc.clone());

        // get provider and node set for startup, we need this so when reth startup
        // happens, we directly can connect to the nodes.

        let startup_provider = ProviderBuilder::<_, _, Ethereum>::default()
            .with_recommended_fillers()
            .connect(&args.boot_node)
            .await
            .unwrap();

        let periphery_c =
            ControllerV1::new(*CONTROLLER_V1_ADDRESS.get().unwrap(), startup_provider);
        let node_set = periphery_c
            .nodes()
            .call()
            .await
            .unwrap()
            .into_iter()
            .collect::<HashSet<_>>();

        if let Some(signer) = args.get_local_signer()? {
            run_with_signer(
                pool,
                executor_clone,
                node_set,
                validation_client,
                quoter_handle,
                consensus_client,
                signer,
                args,
                channels,
                builder
            )
            .await
        } else if let Some(signer) = args.get_hsm_signer()? {
            run_with_signer(
                pool,
                executor_clone,
                node_set,
                validation_client,
                quoter_handle,
                consensus_client,
                signer,
                args,
                channels,
                builder
            )
            .await
        } else {
            unreachable!()
        }
    })
}

async fn run_with_signer<S: AngstromMetaSigner>(
    pool: PoolHandle,
    executor: TaskExecutor,
    node_set: HashSet<Address>,
    validation_client: ValidationClient,
    quoter_handle: QuoterHandle,
    consensus_client: ConsensusHandler,
    secret_key: AngstromSigner<S>,
    args: AngstromConfig,
    mut channels: ConsensusHandles,
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
    let node_handle = builder
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

    AngstromLauncher::<_, _, ConsensusMode, _>::new(
        args,
        executor,
        node_handle,
        secret_key,
        channels
    )
    .with_network(network)
    .with_consensus_client(consensus_client)
    .with_node_set(node_set)
    .launch()
    .await?;

    Ok(())
}
