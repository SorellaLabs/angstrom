//! Angstrom binary executable.
//!
//! ## Feature Flags

use std::path::PathBuf;

use alloy::signers::local::PrivateKeySigner;
use angstrom_metrics::METRICS_ENABLED;
use angstrom_rpc::{OrderApi, api::OrderApiServer};
use angstrom_types::primitive::AngstromSigner;
use clap::Parser;
use cli::AngstromConfig;
use reth::{
    chainspec::EthereumChainSpecParser,
    cli::Cli,
    network::{NetworkProtocols, protocol::IntoRlpxSubProtocol}
};
use reth_node_builder::NodeHandle;
use reth_node_ethereum::EthereumNode;
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

        if args.metrics_enabled {
            executor.spawn_critical("metrics", crate::cli::init_metrics(args.metrics_port));
            METRICS_ENABLED.set(true).unwrap();
        } else {
            METRICS_ENABLED.set(false).unwrap();
        }

        let secret_key = get_secret_key(&args.secret_key_location)?;

        let mut channels = initialize_strom_handles();
        let mut network =
            init_network_builder(secret_key.clone(), channels.eth_handle_rx.take().unwrap())?;
        let protocol_handle = network.build_protocol_handler();

        // for rpc
        let pool = channels.get_pool_handle();
        let executor_clone = executor.clone();
        let validation_client = ValidationClient(channels.validator_tx.clone());

        let NodeHandle { node, node_exit_future } = builder
            .node(EthereumNode::default())
            .extend_rpc_modules(move |rpc_context| {
                let order_api = OrderApi::new(pool.clone(), executor_clone, validation_client);
                rpc_context.modules.merge_configured(order_api.into_rpc())?;

                Ok(())
            })
            .launch()
            .await?;

        node.network
            .add_rlpx_sub_protocol(protocol_handle.into_rlpx_sub_protocol());

        initialize_strom_components(args, secret_key, channels, network, node, &executor).await;

        node_exit_future.await
    })
}

fn get_secret_key(sk_path: &PathBuf) -> eyre::Result<AngstromSigner> {
    let exists = sk_path.try_exists();

    match exists {
        Ok(true) => {
            let contents = std::fs::read_to_string(sk_path)?;
            Ok(AngstromSigner::new(contents.trim().parse::<PrivateKeySigner>()?))
        }
        _ => Err(eyre::eyre!("no secret_key was found at {:?}", sk_path))
    }
}
