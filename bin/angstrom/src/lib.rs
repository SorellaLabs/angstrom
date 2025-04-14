//! Angstrom binary executable.
//!
//! ## Feature Flags

use std::{collections::HashSet, path::PathBuf, sync::Arc};

use alloy::{
    providers::{ProviderBuilder, network::Ethereum},
    signers::local::PrivateKeySigner
};
use angstrom_metrics::METRICS_ENABLED;
use angstrom_network::AngstromNetworkBuilder;
use angstrom_rpc::{OrderApi, api::OrderApiServer};
use angstrom_types::{
    contract_bindings::controller_v_1::ControllerV1,
    primitive::{ANGSTROM_DOMAIN, AngstromSigner}
};
use clap::Parser;
use cli::AngstromConfig;
use parking_lot::RwLock;
use reth::{chainspec::EthereumChainSpecParser, cli::Cli};
use reth_node_builder::{Node, NodeHandle};
use reth_node_ethereum::node::{EthereumAddOns, EthereumNode};
use validation::validator::ValidationClient;

use crate::{
    cli::NodeConfig,
    components::{init_network_builder, initialize_strom_components, initialize_strom_handles}
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

        tracing::info!(domain=?ANGSTROM_DOMAIN);

        let secret_key = get_secret_key(&args.secret_key_location)?;

        let mut channels = initialize_strom_handles();

        // for rpc
        let pool = channels.get_pool_handle();
        let executor_clone = executor.clone();
        let validation_client = ValidationClient(channels.validator_tx.clone());

        // get provider and node set for startup, we need this so when reth startup
        // happens, we directly can connect to the nodes.

        let startup_provider = ProviderBuilder::<_, _, Ethereum>::default()
            .with_recommended_fillers()
            .connect(&args.boot_node)
            .await
            .unwrap();

        let node_config = NodeConfig::load_from_config(Some(args.node_config.clone())).unwrap();

        let periphery_c = ControllerV1::new(node_config.periphery_address, startup_provider);
        let node_set = periphery_c
            .nodes()
            .call()
            .await
            .unwrap()
            ._0
            .into_iter()
            .collect::<HashSet<_>>();

        let mut network = init_network_builder(
            secret_key.clone(),
            channels.eth_handle_rx.take().unwrap(),
            Arc::new(RwLock::new(node_set.clone()))
        )?;

        let protocol_handle = network.build_protocol_handler();

        let NodeHandle { node, node_exit_future } = builder
            .with_types::<EthereumNode>()
            .with_components(
                EthereumNode::default()
                    .components_builder()
                    .network(AngstromNetworkBuilder::new(protocol_handle))
            )
            .with_add_ons::<EthereumAddOns<_>>(Default::default())
            .extend_rpc_modules(move |rpc_context| {
                let order_api = OrderApi::new(pool.clone(), executor_clone, validation_client);
                rpc_context.modules.merge_configured(order_api.into_rpc())?;

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
            node_config
        )
        .await
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
