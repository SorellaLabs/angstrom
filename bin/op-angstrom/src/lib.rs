//! Angstrom binary executable.
//!
//! ## Feature Flags

use std::{collections::HashSet, sync::Arc};

use alloy::providers::{ProviderBuilder, network::Ethereum};
use alloy_chains::NamedChain;
use alloy_primitives::Address;
use angstrom_amm_quoter::QuoterHandle;
use angstrom_metrics::METRICS_ENABLED;
use angstrom_rpc::{OrderApi, api::OrderApiServer};
use angstrom_types::{
    contract_bindings::controller_v_1::ControllerV1,
    primitive::{
        ANGSTROM_DOMAIN, AngstromMetaSigner, AngstromSigner, CONTROLLER_V1_ADDRESS,
        init_with_chain_id
    }
};
use clap::Parser;
use cli::AngstromConfig;
use pool_manager::PoolHandle;
use reth::{chainspec::EthChainSpec, tasks::TaskExecutor};
use reth_db::DatabaseEnv;
use reth_node_builder::{Node, NodeBuilder, NodeHandle, WithLaunchContext};
use reth_optimism_chainspec::OpChainSpec;
use reth_optimism_cli::{Cli as OpCli, chainspec::OpChainSpecParser};
use reth_optimism_node::{OpAddOns, OpNode};
use reth_optimism_primitives::OpPrimitives;
use validation::validator::ValidationClient;

use crate::components::{StromHandles, initialize_strom_components, initialize_strom_handles};

pub mod cli;
pub mod components;

/// Chains supported by op-angstrom.
const SUPPORTED_CHAINS: &[NamedChain] =
    &[NamedChain::Base, NamedChain::BaseSepolia, NamedChain::Unichain, NamedChain::UnichainSepolia];

/// Convenience function for parsing CLI options, set up logging and run the
/// chosen command.
#[inline]
pub fn run() -> eyre::Result<()> {
    // TODO: This should also contain rollup args.
    OpCli::<OpChainSpecParser, AngstromConfig>::parse().run(|builder, args| async move {
        let executor = builder.task_executor().clone();
        let chain = builder.config().chain.chain().named().unwrap();

        let chain = SUPPORTED_CHAINS
            .iter()
            .find(|c| *c == &chain)
            .cloned()
            .expect(
                "op-angstrom does not support chain {chain} (supported chains: \
                 {SUPPORTED_CHAINS:?})"
            );

        init_with_chain_id(chain as u64);

        if args.metrics_enabled {
            executor.spawn_critical("metrics", crate::cli::init_metrics(args.metrics_port));
            METRICS_ENABLED.set(true).unwrap();
        } else {
            METRICS_ENABLED.set(false).unwrap();
        }

        tracing::info!(domain=?ANGSTROM_DOMAIN);

        let channels = initialize_strom_handles();
        let quoter_handle = QuoterHandle(channels.quoter_tx.clone());

        // for rpc
        let pool = channels.get_pool_handle();
        let executor_clone = executor.clone();
        let validation_client = ValidationClient(channels.validator_tx.clone());

        // get provider and node set for startup, we need this so when reth startup
        // happens, we directly can connect to the nodes.

        // TODO: Use different Network generic for OP-stack?
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
    secret_key: AngstromSigner<S>,
    args: AngstromConfig,
    channels: StromHandles<OpPrimitives>,
    builder: WithLaunchContext<NodeBuilder<Arc<DatabaseEnv>, OpChainSpec>>
) -> eyre::Result<()> {
    let executor_clone = executor.clone();
    let NodeHandle { node, node_exit_future } = builder
        .with_types::<OpNode>()
        .with_components(OpNode::default().components_builder())
        .with_add_ons::<OpAddOns<_, _, _, _>>(Default::default())
        .extend_rpc_modules(move |rpc_context| {
            let order_api = OrderApi::new(
                pool.clone(),
                executor_clone.clone(),
                validation_client,
                quoter_handle
            );
            rpc_context.modules.merge_configured(order_api.into_rpc())?;

            Ok(())
        })
        .launch()
        .await?;

    initialize_strom_components(
        args,
        secret_key,
        channels,
        &node,
        executor,
        node_exit_future,
        node_set
    )
    .await
}
