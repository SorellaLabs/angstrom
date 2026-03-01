use std::path::PathBuf;

use alloy::signers::local::PrivateKeySigner;
use angstrom_metrics::initialize_prometheus_metrics;
use angstrom_types::primitive::{
    AngstromSigner, CHAIN_ID, ETH_ANGSTROM_RPC, ETH_DEFAULT_RPC, ETH_MEV_RPC
};
use consensus::ConsensusTimingConfig;
use hsm_signer::{Pkcs11Signer, Pkcs11SignerConfig};

#[derive(Debug, Clone, Default, clap::Args)]
pub struct AngstromConfig {
    /// enables the metrics
    #[clap(long, default_value = "false", global = true)]
    pub metrics_enabled:           bool,
    /// spawns the prometheus metrics exporter at the specified port
    /// Default: 6969
    #[clap(long, default_value = "6969", global = true)]
    pub metrics_port:              u16,
    #[clap(short, long, num_args(0..=10), require_equals = true, default_values = ETH_MEV_RPC)]
    pub mev_boost_endpoints:       Vec<String>,
    /// needed to properly setup the node as we need some chain state before
    /// starting the internal reth node
    #[clap(short, long, default_value = "https://eth.drpc.org")]
    pub boot_node:                 String,
    #[clap(short, long, num_args(0..=5), require_equals = true, default_values = ETH_DEFAULT_RPC)]
    pub normal_nodes:              Vec<String>,
    #[clap(short, long, num_args(0..=10), require_equals = true, default_values = ETH_ANGSTROM_RPC)]
    pub angstrom_submission_nodes: Vec<String>,
    #[clap(flatten)]
    pub key_config:                KeyConfig,
    #[clap(flatten)]
    pub consensus_timing:          ConsensusTimingConfig
}

impl AngstromConfig {
    pub fn get_local_signer(&self) -> eyre::Result<Option<AngstromSigner<PrivateKeySigner>>> {
        self.key_config
            .local_secret_key_location
            .as_ref()
            .map(|sk_path| {
                if sk_path.try_exists()? {
                    let contents = std::fs::read_to_string(sk_path)?;
                    Ok(AngstromSigner::new(contents.trim().parse::<PrivateKeySigner>()?))
                } else {
                    Err(eyre::eyre!("no secret_key was found at {:?}", sk_path))
                }
            })
            .transpose()
    }

    pub fn get_hsm_signer(&self) -> eyre::Result<Option<AngstromSigner<Pkcs11Signer>>> {
        Ok((self.key_config.hsm_enabled)
            .then(|| {
                Pkcs11Signer::new(
                    Pkcs11SignerConfig::from_env_with_defaults(
                        self.key_config.hsm_public_key_label.as_ref().unwrap(),
                        self.key_config.hsm_private_key_label.as_ref().unwrap(),
                        self.key_config.pkcs11_lib_path.clone().into(),
                        None
                    ),
                    Some(*CHAIN_ID.get().unwrap())
                )
                .map(AngstromSigner::new)
            })
            .transpose()?)
    }
}

pub async fn init_metrics(metrics_port: u16) {
    let _ = initialize_prometheus_metrics(metrics_port)
        .await
        .inspect_err(|e| eprintln!("failed to start metrics endpoint - {e:?}"));
}

#[derive(Debug, Clone, Default, clap::Args)]
pub struct KeyConfig {
    #[clap(long, conflicts_with = "hsm_enabled")]
    pub local_secret_key_location: Option<PathBuf>,
    #[clap(long, conflicts_with = "local_secret_key_location")]
    pub hsm_enabled:               bool,
    #[clap(long, requires = "hsm_enabled")]
    pub hsm_public_key_label:      Option<String>,
    #[clap(long, requires = "hsm_enabled")]
    pub hsm_private_key_label:     Option<String>,
    #[clap(
        long,
        requires = "hsm_enabled",
        default_value = "/opt/cloudhsm/lib/libcloudhsm_pkcs11.so"
    )]
    pub pkcs11_lib_path:           String
}
