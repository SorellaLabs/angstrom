use std::path::PathBuf;

use alloy_primitives::Address;
use angstrom_metrics::initialize_prometheus_metrics;
use angstrom_types::contract_bindings::angstrom::Angstrom::PoolKey;
use eyre::Context;
use serde::Deserialize;

#[derive(Debug, Clone, Default, clap::Args)]
pub struct AngstromConfig {
    #[clap(long)]
    pub mev_guard:           bool,
    #[clap(long)]
    pub secret_key_location: PathBuf,
    #[clap(long)]
    pub angstrom_addr:       Option<Address>,
    #[clap(long)]
    pub node_config:         PathBuf,
    /// enables the metrics
    #[clap(long, default_value = "false", global = true)]
    pub metrics:             bool,
    /// spawns the prometheus metrics exporter at the specified port
    /// Default: 6969
    #[clap(long, default_value = "6969", global = true)]
    pub metrics_port:        u16
}

#[derive(Debug, Clone, Deserialize)]
pub struct NodeConfig {
    pub secret_key:       String,
    pub angstrom_address: Address,
    pub pools:            Vec<PoolKey>
}

impl NodeConfig {
    pub fn load_from_config(config: Option<PathBuf>) -> Result<Self, eyre::Report> {
        let config_path = config.ok_or_else(|| eyre::eyre!("Config path not provided"))?;

        if !config_path.exists() {
            return Err(eyre::eyre!("Config file does not exist at {:?}", config_path))
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
