use std::path::PathBuf;

use alloy::signers::local::PrivateKeySigner;
use angstrom_types::primitive::{
    AngstromSigner, CHAIN_ID, ETH_ANGSTROM_RPC, ETH_DEFAULT_RPC, ETH_MEV_RPC
};
use consensus::ConsensusTimingConfig;
use hsm_signer::{Pkcs11Signer, Pkcs11SignerConfig};
use url::Url;

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
    #[clap(short, long, num_args(1..=5), require_equals = true)]
    pub normal_nodes:              Option<Vec<String>>,
    #[clap(short, long, num_args(0..=10), require_equals = true, default_values = ETH_ANGSTROM_RPC)]
    pub angstrom_submission_nodes: Vec<String>,
    #[clap(flatten)]
    pub key_config:                KeyConfig,
    /// The expected block time in milliseconds. Used to coordinate auction
    /// timing.
    #[clap(long, default_value_t = 12000, global = true, alias = "block-time")]
    pub block_time_ms:             u64,
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

    /// Get normal nodes from config or default to ETH_DEFAULT_RPC
    pub fn get_normal_nodes(&self) -> Vec<Url> {
        self.normal_nodes
            .as_ref()
            .map(|v| v.iter().map(|s| Url::parse(s).unwrap()).collect())
            .unwrap_or_else(|| {
                ETH_DEFAULT_RPC
                    .iter()
                    .map(|s| Url::parse(s).unwrap())
                    .collect()
            })
    }

    /// Add sequencer to normal nodes if it's not already in the list
    pub fn add_sequencer(&mut self, sequencer: &str) -> eyre::Result<()> {
        match &mut self.normal_nodes {
            Some(nodes) => {
                if !nodes.contains(&sequencer.to_string()) {
                    nodes.push(sequencer.to_string());
                }
            }
            None => {
                self.normal_nodes = Some(vec![sequencer.to_string()]);
            }
        }

        Ok(())
    }

    /// Validate sequencer URL format
    pub fn validate_sequencer_url(sequencer: &str) -> eyre::Result<()> {
        let url = Url::parse(sequencer)?;
        match url.scheme() {
            "ws" | "wss" => Err(eyre::eyre!("Sequencer URL must be HTTP, not WS")),
            _ => Ok(())
        }
    }

    /// Validate and add sequencer to normal nodes
    pub fn add_validated_sequencer(&mut self, sequencer: &str) -> eyre::Result<()> {
        Self::validate_sequencer_url(sequencer)?;
        self.add_sequencer(sequencer)?;
        Ok(())
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_sequencer_none() {
        let mut cfg = AngstromConfig { normal_nodes: None, ..Default::default() };
        cfg.add_sequencer("node1").unwrap();
        assert_eq!(cfg.normal_nodes.unwrap(), vec!["node1"]);
    }

    #[test]
    fn add_sequencer_some_without_sequencer() {
        let mut cfg =
            AngstromConfig { normal_nodes: Some(vec!["node2".into()]), ..Default::default() };
        cfg.add_sequencer("node1").unwrap();
        assert_eq!(cfg.normal_nodes.unwrap(), vec!["node2", "node1"]);
    }

    #[test]
    fn add_sequencer_some_with_sequencer() {
        let mut cfg =
            AngstromConfig { normal_nodes: Some(vec!["node1".into()]), ..Default::default() };
        cfg.add_sequencer("node1").unwrap();
        assert_eq!(cfg.normal_nodes.unwrap(), vec!["node1"]);
    }

    #[test]
    fn add_sequencer_appends_new() {
        let mut cfg = AngstromConfig {
            normal_nodes: Some(vec!["a".into(), "b".into()]),
            ..Default::default()
        };
        cfg.add_sequencer("c").unwrap();
        assert_eq!(cfg.normal_nodes.unwrap(), vec!["a", "b", "c"]);
    }

    #[test]
    fn validate_sequencer_url_accepts_http() {
        for url in [
            "http://example.com",
            "https://example.com",
            "https://example.com:8080/path",
            "https://example.com/path?x=1#frag"
        ] {
            assert!(AngstromConfig::validate_sequencer_url(url).is_ok(), "accepted: {url}");
        }
    }

    #[test]
    fn validate_sequencer_url_rejects_websocket() {
        for url in ["ws://example.com", "wss://example.com", "WS://EXAMPLE.COM", "Wss://x.y"] {
            let err = AngstromConfig::validate_sequencer_url(url).unwrap_err();
            assert_eq!(err.to_string(), "Sequencer URL must be HTTP, not WS", "url={url}");
        }
    }

    #[test]
    fn validate_sequencer_url_invalids() {
        for url in ["", "not-a-url", "://invalid", "http://"] {
            assert!(
                AngstromConfig::validate_sequencer_url(url).is_err(),
                "invalid accepted: {url}"
            );
        }
    }

    #[test]
    fn add_validated_sequencer_success() {
        let mut cfg = AngstromConfig { normal_nodes: None, ..Default::default() };
        cfg.add_validated_sequencer("https://example.com").unwrap();
        assert_eq!(cfg.normal_nodes.unwrap(), vec!["https://example.com"]);
    }

    #[test]
    fn add_validated_sequencer_rejects_websocket() {
        let mut cfg = AngstromConfig { normal_nodes: None, ..Default::default() };
        let err = cfg.add_validated_sequencer("ws://example.com").unwrap_err();
        assert_eq!(err.to_string(), "Sequencer URL must be HTTP, not WS");
        assert_eq!(cfg.normal_nodes, None); // Should not be modified on error
    }
}
