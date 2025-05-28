use std::path::PathBuf;

use alloy_primitives::Address;
use angstrom_metrics::initialize_prometheus_metrics;
use angstrom_types::primitive::{ANGSTROM_RPC, AngstromSigner, DEFAULT_RPC, MEV_RPC};
use eyre::Context;
use serde::Deserialize;
use url::Url;

#[derive(Debug, Clone, Default, clap::Args)]
pub struct AngstromConfig {
    #[cfg(not(feature = "aws-signer"))]
    #[clap(long)]
    pub secret_key_location:       PathBuf,
    #[cfg(feature = "aws-signer")]
    #[clap(long)]
    pub aws_region:                String,
    #[cfg(feature = "aws-signer")]
    #[clap(long)]
    pub aws_kms_node_pk_id:        String,
    #[clap(long)]
    pub node_config:               PathBuf,
    /// enables the metrics
    #[clap(long, default_value = "false", global = true)]
    pub metrics_enabled:           bool,
    /// spawns the prometheus metrics exporter at the specified port
    /// Default: 6969
    #[clap(long, default_value = "6969", global = true)]
    pub metrics_port:              u16,
    #[clap(short, long, default_values = MEV_RPC)]
    pub mev_boost_endpoints:       Vec<Url>,
    /// needed to properly setup the node as we need some chain state before
    /// starting the internal reth node
    #[clap(short, long, default_value = "https://eth.drpc.org")]
    pub boot_node:                 String,
    #[clap(
        short,
        long,
        default_values = DEFAULT_RPC
    )]
    pub normal_nodes:              Vec<Url>,
    #[clap(
        short,
        long,
        default_values = ANGSTROM_RPC
    )]
    pub angstrom_submission_nodes: Vec<Url>
}

impl AngstromConfig {
    #[cfg(not(feature = "aws-signer"))]
    pub fn get_secret_key(&self) -> eyre::Result<AngstromSigner> {
        let sk_path = &self.secret_key_location;
        let exists = sk_path.try_exists();

        match exists {
            Ok(true) => {
                let contents = std::fs::read_to_string(sk_path)?;
                Ok(AngstromSigner::new(
                    contents
                        .trim()
                        .parse::<alloy::signers::local::PrivateKeySigner>()?
                ))
            }
            _ => Err(eyre::eyre!("no secret_key was found at {:?}", sk_path))
        }
    }

    #[cfg(feature = "aws-signer")]
    pub async fn get_secret_key(
        &self,
        handle: tokio::runtime::Handle
    ) -> eyre::Result<AngstromSigner> {
        use alloy::signers::aws::AwsSigner;
        use angstrom_types::primitive::AngstromAwsSigner;
        use aws_config::{BehaviorVersion, Region};

        let mut cfg_builder = aws_config::load_defaults(BehaviorVersion::latest())
            .await
            .into_builder();
        cfg_builder.set_region(Some(Region::new(self.aws_region.clone())));
        let cfg = cfg_builder.build();

        let client = aws_sdk_kms::Client::new(&cfg);

        let signer =
            AwsSigner::new(client, self.aws_kms_node_pk_id.clone().into(), Some(1)).await?;

        Ok(AngstromSigner::new(AngstromAwsSigner::new(signer, handle)))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct NodeConfig {
    pub angstrom_address:     Address,
    pub periphery_address:    Address,
    pub pool_manager_address: Address,
    pub gas_token_address:    Address,

    pub angstrom_deploy_block: u64
}

impl NodeConfig {
    pub fn load_from_config(config: Option<PathBuf>) -> Result<Self, eyre::Report> {
        let config_path = config.ok_or_else(|| eyre::eyre!("Config path not provided"))?;

        if !config_path.exists() {
            return Err(eyre::eyre!("Config file does not exist at {:?}", config_path));
        }

        let toml_content = std::fs::read_to_string(&config_path)
            .wrap_err_with(|| format!("Could not read config file {:?}", config_path))?;

        let node_config: NodeConfig = toml::from_str(&toml_content)
            .wrap_err_with(|| format!("Could not deserialize config file {:?}", config_path))?;

        Ok(node_config)
    }
}

pub async fn init_metrics(metrics_port: u16) {
    let _ = initialize_prometheus_metrics(metrics_port)
        .await
        .inspect_err(|e| eprintln!("failed to start metrics endpoint - {:?}", e));
}
