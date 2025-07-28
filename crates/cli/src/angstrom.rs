//! Angstrom binary executable.
use std::collections::HashSet;

use alloy::providers::{ProviderBuilder, network::Ethereum};
use alloy_chains::NamedChain;
use angstrom_amm_quoter::QuoterHandle;
use angstrom_metrics::METRICS_ENABLED;
use angstrom_types::{
    contract_bindings::controller_v_1::ControllerV1,
    primitive::{ANGSTROM_DOMAIN, CONTROLLER_V1_ADDRESS, init_with_chain_id}
};
use clap::Parser;
use consensus::ConsensusHandler;
use reth::{
    chainspec::{EthChainSpec, EthereumChainSpecParser},
    cli::Cli
};
use validation::validator::ValidationClient;

use crate::{
    components::initialize_strom_handles, config::AngstromConfig, metrics::init_metrics,
    run_with_signer
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

        let channels = initialize_strom_handles();
        let quoter_handle = QuoterHandle(channels.quoter_tx.clone());

        // for rpc
        let pool = channels.get_pool_handle();
        let executor_clone = executor.clone();
        let validation_client = ValidationClient(channels.validator_tx.clone());
        let consensus_client = ConsensusHandler(channels.consensus_tx_rpc.clone());

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
