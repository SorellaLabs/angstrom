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
use url::Url;
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
pub struct OpAngstromArgs {
    #[command(flatten)]
    pub rollup:      RollupArgs,
    #[command(flatten)]
    pub angstrom:    AngstromConfig,
    #[command(flatten)]
    pub flashblocks: FlashblocksConfig
}

pub struct FlashblocksConfig {
    /// Enable Flashblocks support.
    #[arg(long = "flashblocks", default_value = "false")]
    pub enabled: bool,

    /// Flashblocks WebSocket URL.
    #[arg(long = "flashblocks.ws", value_name = "WEBSOCKET_URL")]
    pub url: Option<String>
}

impl OpAngstromArgs {
    fn flashblocks_enabled(&self) -> bool {
        self.flashblocks.enabled
    }
}

/// Convenience function for parsing CLI options, set up logging and run the
/// chosen command.
#[inline]
pub fn run() -> eyre::Result<()> {
    OpCli::<OpChainSpecParser, OpAngstromArgs>::parse().run(|builder, mut args| async move {
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

        // --rollup.sequencer must be provided.
        let Some(sequencer) = args.rollup.sequencer.as_ref() else {
            return Err(eyre::eyre!("Missing required flag --rollup.sequencer"));
        };

        if let Ok(url) = Url::parse(sequencer) {
            if matches!(url.scheme(), "ws" | "wss") {
                return Err(eyre::eyre!("Sequencer URL must be HTTP, not WS"));
            }
        }

        let l2_url = sequencer.clone();

        // Add sequencer to normal nodes
        if let Some(ref mut nodes) = args.angstrom.normal_nodes {
            // Don't add the the endpoint if it's already in the list
            if !nodes.contains(&l2_url) {
                nodes.push(l2_url);
            }
        } else {
            args.angstrom.normal_nodes = Some(vec![l2_url]);
        }

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
    args: OpAngstromArgs,
    channels: RollupHandles,
    builder: WithLaunchContext<NodeBuilder<Arc<DatabaseEnv>, OpChainSpec>>
) -> eyre::Result<()> {
    let executor_clone = executor.clone();

    let op_node = OpNode::new(args.rollup);

    let node_handle = builder
        .with_types::<OpNode>()
        .with_components(op_node.components_builder())
        .with_add_ons(op_node.add_ons())
        .install_exex_if(args.flashblocks_enabled(), "flashblocks", {
            // TODO: Install Flashblocks ExEx here
            todo!();
        })
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
