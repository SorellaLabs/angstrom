//! Angstrom binary executable.
//!
//! ## Feature Flags

use angstrom_metrics::METRICS_ENABLED;
use angstrom_network::AngstromNetworkBuilder;
use angstrom_rpc::{api::OrderApiServer, OrderApi};
use clap::Parser;
use cli::AngstromConfig;
use reth::{chainspec::EthereumChainSpecParser, cli::Cli};
use reth_cli_util::get_secret_key;
use reth_node_builder::{Node, NodeHandle};
use reth_node_ethereum::{node::EthereumAddOns, EthereumNode};
use validation::validator::ValidationClient;

use crate::components::{
    init_network_builder, initialize_strom_components, initialize_strom_handles
};

pub mod cli;
pub mod components;

/// Convenience function for parsing CLI options, set up logging and run the
/// chosen command.
#[inline]
pub fn run() -> eyre::Result<()> {
    Cli::<EthereumChainSpecParser, AngstromConfig>::parse().run(|builder, args| async move {
        let executor = builder.task_executor().clone();

        if args.metrics {
            executor.spawn_critical("metrics", crate::cli::init_metrics(args.metrics_port));
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
        let validation_client = ValidationClient(channels.validator_tx.clone());
        let NodeHandle { node, node_exit_future } = builder
            .with_types::<EthereumNode>()
            .with_components(
                EthereumNode::default()
                    .components_builder()
                    .network(AngstromNetworkBuilder::new(protocol_handle))
            )
            .with_add_ons::<EthereumAddOns>(Default::default())
            .extend_rpc_modules(move |rpc_context| {
                let order_api = OrderApi::new(pool.clone(), executor_clone, validation_client);
                rpc_context.modules.merge_configured(order_api.into_rpc())?;

                Ok(())
            })
            .launch()
            .await?;

        initialize_strom_components(
            args,
            secret_key,
            channels,
            network,
            node,
            &executor
        )
        .await;

        node_exit_future.await
    })
}
