use std::{net::IpAddr, path::PathBuf, str::FromStr};

use alloy::signers::local::PrivateKeySigner;
use alloy_primitives::{Address, Bytes};
use alloy_signer_local::LocalSigner;
use angstrom_metrics::{initialize_prometheus_metrics, METRICS_ENABLED};
use angstrom_types::contract_bindings::angstrom::Angstrom::PoolKey;
use consensus::AngstromValidator;
use enr::k256::ecdsa::SigningKey;
use eyre::Context;
use reth_network_peers::pk2id;
use secp256k1::{Secp256k1, SecretKey};
use serde::Deserialize;

#[derive(Debug, Clone, Default, clap::Parser)]
pub struct AngstromTestnetCli {
    #[clap(long)]
    pub mev_guard:    bool,
    #[clap(long, default_value = "./testnet_config.toml")]
    pub node_config:  PathBuf,
    /// enables the metrics
    #[clap(long, default_value = "false", global = true)]
    pub metrics:      bool,
    /// spawns the prometheus metrics exporter at the specified port
    /// Default: 6969
    #[clap(long, default_value = "6969", global = true)]
    pub metrics_port: u16
}

impl AngstromTestnetCli {
    pub async fn init_metrics(self) {
        if self.metrics
            && initialize_prometheus_metrics(self.metrics_port)
                .await
                .inspect_err(|e| eprintln!("failed to start metrics endpoint - {:?}", e))
                .is_ok()
        {
            {
                METRICS_ENABLED.set(true).unwrap();
            }
        } else {
            METRICS_ENABLED.set(false).unwrap();
        }
    }

    pub(crate) fn load_config(&self) -> eyre::Result<FullTestnetNodeConfig> {
        FullTestnetNodeConfig::load_from_config(&self.node_config)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FullTestnetNodeConfig {
    pub nodes:                Vec<TestnetNodeConfig>,
    pub angstrom_address:     Address,
    pub pool_manager_address: Address,
    pub pools_keys:           Vec<PoolKey>
}

impl FullTestnetNodeConfig {
    fn load_from_config(config_path: &PathBuf) -> eyre::Result<Self> {
        FullTestnetNodeConfigInner::load_from_config(config_path)?.try_into()
    }

    pub fn my_node_config(&self) -> eyre::Result<TestnetNodeConfig> {
        let my_ip = local_ip_address::local_ip()?;
        self.nodes
            .iter()
            .find(|node| node.ip == my_ip)
            .cloned()
            .ok_or(eyre::eyre!("no node found for IP: {my_ip:?}"))
    }

    pub fn leader_ws_url(&self) -> eyre::Result<String> {
        self.nodes
            .iter()
            .find(|node| node.is_leader)
            .map(|node| format!("ws://{}:8545", node.ip))
            .ok_or(eyre::eyre!("no leader node found"))
    }

    pub fn initial_validators(&self) -> Vec<AngstromValidator> {
        self.nodes.iter().map(|node| node.clone().into()).collect()
    }
}

impl TryFrom<FullTestnetNodeConfigInner> for FullTestnetNodeConfig {
    type Error = eyre::ErrReport;

    fn try_from(value: FullTestnetNodeConfigInner) -> Result<Self, Self::Error> {
        Ok(FullTestnetNodeConfig {
            nodes:                value
                .nodes
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<_>, _>>()?,
            pools_keys:           value.pools_keys.unwrap_or_default(),
            pool_manager_address: Address::from_str(&value.pool_manager_address)?,
            angstrom_address:     Address::from_str(&value.angstrom_address)?
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TestnetNodeConfig {
    pub address:     Address,
    pub ip:          IpAddr,
    pub port:        u16,
    pub is_leader:   bool,
    pub signing_key: PrivateKeySigner,
    pub secret_key:  SecretKey
}

impl TryFrom<TestnetNodeConfigInner> for TestnetNodeConfig {
    type Error = eyre::ErrReport;

    fn try_from(value: TestnetNodeConfigInner) -> Result<Self, Self::Error> {
        let ip = IpAddr::from_str(&value.ip)?;
        let secret_key = SecretKey::from_slice(&Bytes::from_str(&value.secret_key)?.0.to_vec())?;
        let signing_key =
            LocalSigner::<SigningKey>::from_bytes(&secret_key.secret_bytes().into()).unwrap();

        let address = signing_key.address();

        Ok(TestnetNodeConfig {
            address,
            ip,
            port: value.port.unwrap_or(4200),
            is_leader: value.is_leader,
            signing_key,
            secret_key
        })
    }
}

impl Into<AngstromValidator> for TestnetNodeConfig {
    fn into(self) -> AngstromValidator {
        let pub_key = self.secret_key.public_key(&Secp256k1::default());

        AngstromValidator::new(pk2id(&pub_key), 1)
    }
}

#[derive(Debug, Clone, Deserialize)]
struct FullTestnetNodeConfigInner {
    nodes:                Vec<TestnetNodeConfigInner>,
    pools_keys:           Option<Vec<PoolKey>>,
    angstrom_address:     String,
    pool_manager_address: String
}

impl FullTestnetNodeConfigInner {
    fn load_from_config(config_path: &PathBuf) -> eyre::Result<Self> {
        if !config_path.exists() {
            return Err(eyre::eyre!("Config file does not exist at {:?}", config_path))
        }

        let toml_content = std::fs::read_to_string(&config_path)
            .wrap_err_with(|| format!("Could not read config file {:?}", config_path))?;

        let node_config: Self = toml::from_str(&toml_content)
            .wrap_err_with(|| format!("Could not deserialize config file {:?}", config_path))?;

        Ok(node_config)
    }
}

#[derive(Debug, Clone, Deserialize)]
struct TestnetNodeConfigInner {
    ip:         String,
    port:       Option<u16>,
    is_leader:  bool,
    secret_key: String
}

mod tests {
    use super::*;

    #[test]
    fn test_read_config() {
        let path = PathBuf::from_str("./testnet_config.toml").unwrap();
        println!("{:?}", path);

        let config = FullTestnetNodeConfigInner::load_from_config(&path);
        config.as_ref().unwrap();
        assert!(config.is_ok());
    }
}
