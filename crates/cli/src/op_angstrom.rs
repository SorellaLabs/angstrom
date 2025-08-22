//! Optimism Angstrom binary executable.
use std::sync::Arc;

use alloy_chains::NamedChain;
use angstrom_amm_quoter::QuoterHandle;
use angstrom_metrics::METRICS_ENABLED;
use angstrom_rpc::{OrderApi, api::OrderApiServer};
use angstrom_types::primitive::{
    ANGSTROM_DOMAIN, AngstromMetaSigner, AngstromSigner, init_with_chain_id
};
use clap::Parser;
use pool_manager::PoolHandle;
use reth::{chainspec::EthChainSpec, tasks::TaskExecutor};
use reth_db::DatabaseEnv;
use reth_node_builder::{Node, NodeBuilder, WithLaunchContext};
use reth_optimism_chainspec::OpChainSpec;
use reth_optimism_cli::{Cli as OpCli, chainspec::OpChainSpecParser};
use reth_optimism_node::{OpAddOns, OpNode, args::RollupArgs};
use validation::validator::ValidationClient;

use crate::{
    components::AngstromLauncher,
    config::AngstromConfig,
    handles::{RollupHandles, RollupMode},
    metrics::init_metrics
};

/// Chains supported by op-angstrom.
const SUPPORTED_CHAINS: &[NamedChain] =
    &[NamedChain::Base, NamedChain::BaseSepolia, NamedChain::Unichain, NamedChain::UnichainSepolia];

#[derive(Debug, Clone, Parser)]
pub struct CombinedArgs {
    #[command(flatten)]
    pub rollup:   RollupArgs,
    #[command(flatten)]
    pub angstrom: AngstromConfig
}

/// Convenience function for parsing CLI options, set up logging and run the
/// chosen command.
#[inline]
pub fn run() -> eyre::Result<()> {
    // TODO: This should also contain rollup args.
    OpCli::<OpChainSpecParser, CombinedArgs>::parse().run(|builder, args| async move {
        let executor = builder.task_executor().clone();
        let chain = builder.config().chain.chain().named().unwrap();

        let chain = SUPPORTED_CHAINS
            .iter()
            .find(|c| *c == &chain)
            .cloned()
            .expect(&format!(
                "op-angstrom does not support chain {chain} (supported chains: \
                 {SUPPORTED_CHAINS:?})"
            ));

        init_with_chain_id(chain as u64);

        if args.angstrom.metrics_enabled {
            executor.spawn_critical("metrics", init_metrics(args.angstrom.metrics_port));
            METRICS_ENABLED.set(true).unwrap();
        } else {
            METRICS_ENABLED.set(false).unwrap();
        }

        tracing::info!(domain=?ANGSTROM_DOMAIN);

        let channels = RollupHandles::new();
        let quoter_handle = QuoterHandle(channels.quoter_tx.clone());

        // for rpc
        let pool = channels.get_pool_handle();
        let executor_clone = executor.clone();
        let validation_client = ValidationClient(channels.validator_tx.clone());

        if let Some(signer) = args.angstrom.get_local_signer()? {
            run_with_signer(
                pool,
                executor_clone,
                validation_client,
                quoter_handle,
                signer,
                args,
                channels,
                builder
            )
            .await
        } else if let Some(signer) = args.angstrom.get_hsm_signer()? {
            run_with_signer(
                pool,
                executor_clone,
                validation_client,
                quoter_handle,
                signer,
                args,
                channels,
                builder
            )
            .await
        } else {
            panic!("No signer provided");
        }
    })
}

async fn run_with_signer<S: AngstromMetaSigner>(
    pool: PoolHandle,
    executor: TaskExecutor,
    validation_client: ValidationClient,
    quoter_handle: QuoterHandle,
    secret_key: AngstromSigner<S>,
    args: CombinedArgs,
    channels: RollupHandles,
    builder: WithLaunchContext<NodeBuilder<Arc<DatabaseEnv>, OpChainSpec>>
) -> eyre::Result<()> {
    let executor_clone = executor.clone();
    let node_handle = builder
        .with_types::<OpNode>()
        .with_components(OpNode::new(args.rollup).components_builder())
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

    AngstromLauncher::<_, _, RollupMode, _>::new(
        args.angstrom,
        executor,
        node_handle,
        secret_key,
        channels
    )
    .launch()
    .await
}
