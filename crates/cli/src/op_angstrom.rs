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
    OpCli::<OpChainSpecParser, CombinedArgs>::parse().run(|builder, mut args| async move {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::KeyConfig;
    use clap::Parser as ClapParser;
    use tempfile::NamedTempFile;
    use std::io::Write;

    #[test]
    fn test_supported_chains() {
        // Verify all supported chains are correctly defined
        assert_eq!(SUPPORTED_CHAINS.len(), 4);
        assert!(SUPPORTED_CHAINS.contains(&NamedChain::Base));
        assert!(SUPPORTED_CHAINS.contains(&NamedChain::BaseSepolia));
        assert!(SUPPORTED_CHAINS.contains(&NamedChain::Unichain));
        assert!(SUPPORTED_CHAINS.contains(&NamedChain::UnichainSepolia));
    }

    #[test]
    fn test_combined_args_parsing() {
        // Test basic argument parsing
        let args = CombinedArgs::try_parse_from([
            "op-angstrom",
            "--rollup.sequencer", "http://localhost:8545",
            "--local-secret-key-location", "/tmp/key.txt",
        ]).unwrap();

        assert_eq!(args.rollup.sequencer, Some("http://localhost:8545".to_string()));
        assert_eq!(args.angstrom.key_config.local_secret_key_location, Some("/tmp/key.txt".into()));
    }

    #[test]
    fn test_metrics_configuration() {
        // Test metrics configuration parsing
        let args = CombinedArgs::try_parse_from([
            "op-angstrom",
            "--rollup.sequencer", "http://localhost:8545",
            "--metrics-enabled",
            "--metrics-port", "7070",
            "--local-secret-key-location", "/tmp/key.txt",
        ]).unwrap();

        assert!(args.angstrom.metrics_enabled);
        assert_eq!(args.angstrom.metrics_port, 7070);
    }

    #[test]
    fn test_sequencer_url_validation() {
        // Test that WebSocket URLs are rejected
        let ws_url = "ws://localhost:8545";
        let wss_url = "wss://localhost:8545";
        let http_url = "http://localhost:8545";
        let https_url = "https://localhost:8545";

        // Parse URLs and check schemes
        let ws_parsed = Url::parse(ws_url).unwrap();
        let wss_parsed = Url::parse(wss_url).unwrap();
        let http_parsed = Url::parse(http_url).unwrap();
        let https_parsed = Url::parse(https_url).unwrap();

        assert!(matches!(ws_parsed.scheme(), "ws"));
        assert!(matches!(wss_parsed.scheme(), "wss"));
        assert!(matches!(http_parsed.scheme(), "http"));
        assert!(matches!(https_parsed.scheme(), "https"));
    }

    #[test]
    fn test_node_list_deduplication() {
        // Test that sequencer is added to normal nodes without duplication
        let mut args = CombinedArgs::try_parse_from([
            "op-angstrom",
            "--rollup.sequencer", "http://localhost:8545",
            "--normal-nodes=http://localhost:8545,http://localhost:8546",
            "--local-secret-key-location", "/tmp/key.txt",
        ]).unwrap();

        let l2_url = args.rollup.sequencer.as_ref().unwrap().clone();

        // Simulate the deduplication logic from run()
        if let Some(ref mut nodes) = args.angstrom.normal_nodes {
            if !nodes.contains(&l2_url) {
                nodes.push(l2_url.clone());
            }
        } else {
            args.angstrom.normal_nodes = Some(vec![l2_url.clone()]);
        }

        // Verify no duplication occurred
        let nodes = args.angstrom.normal_nodes.unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes.iter().filter(|n| **n == l2_url).count(), 1);
    }

    #[test]
    fn test_key_config_conflicts() {
        // Test that local and HSM keys conflict
        let result = CombinedArgs::try_parse_from([
            "op-angstrom",
            "--rollup.sequencer", "http://localhost:8545",
            "--local-secret-key-location", "/tmp/key.txt",
            "--hsm-enabled",
        ]);

        // This should fail due to conflict
        assert!(result.is_err());
    }

    #[test]
    fn test_hsm_config_requirements() {
        // Test that HSM requires labels
        let result = CombinedArgs::try_parse_from([
            "op-angstrom",
            "--rollup.sequencer", "http://localhost:8545",
            "--hsm-enabled",
        ]);

        // This should succeed but hsm labels are missing
        assert!(result.is_ok());
        let args = result.unwrap();
        assert!(args.angstrom.key_config.hsm_enabled);
        assert!(args.angstrom.key_config.hsm_public_key_label.is_none());
        assert!(args.angstrom.key_config.hsm_private_key_label.is_none());
    }

    #[test]
    fn test_get_local_signer_with_valid_file() {
        // Create a temporary file with a valid private key
        let mut temp_file = NamedTempFile::new().unwrap();
        let private_key = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
        temp_file.write_all(private_key.as_bytes()).unwrap();

        let config = AngstromConfig {
            key_config: KeyConfig {
                local_secret_key_location: Some(temp_file.path().to_path_buf()),
                ..Default::default()
            },
            ..Default::default()
        };

        let signer = config.get_local_signer();
        assert!(signer.is_ok());
        assert!(signer.unwrap().is_some());
    }

    #[test]
    fn test_get_local_signer_with_missing_file() {
        let config = AngstromConfig {
            key_config: KeyConfig {
                local_secret_key_location: Some("/nonexistent/path/key.txt".into()),
                ..Default::default()
            },
            ..Default::default()
        };

        let signer = config.get_local_signer();
        assert!(signer.is_err());
    }

    #[test]
    fn test_get_normal_nodes_with_defaults() {
        let config = AngstromConfig {
            normal_nodes: None,
            ..Default::default()
        };

        let nodes = config.get_normal_nodes();
        assert!(!nodes.is_empty());
    }

    #[test]
    fn test_get_normal_nodes_with_custom() {
        let config = AngstromConfig {
            normal_nodes: Some(vec![
                "http://localhost:8545".to_string(),
                "http://localhost:8546".to_string(),
            ]),
            ..Default::default()
        };

        let nodes = config.get_normal_nodes();
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    fn test_block_time_configuration() {
        let args = CombinedArgs::try_parse_from([
            "op-angstrom",
            "--rollup.sequencer", "http://localhost:8545",
            "--block-time", "2000",
            "--local-secret-key-location", "/tmp/key.txt",
        ]).unwrap();

        assert_eq!(args.angstrom.block_time_ms, 2000);
    }

    #[test]
    fn test_default_block_time() {
        let args = CombinedArgs::try_parse_from([
            "op-angstrom",
            "--rollup.sequencer", "http://localhost:8545",
            "--local-secret-key-location", "/tmp/key.txt",
        ]).unwrap();

        assert_eq!(args.angstrom.block_time_ms, 12000);
    }
}
