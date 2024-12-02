use std::{net::IpAddr, path::PathBuf, str::FromStr};

use alloy::signers::local::PrivateKeySigner;
use alloy_primitives::{
    aliases::{I24, U24},
    Address, Bytes
};
use alloy_signer_local::LocalSigner;
use angstrom_metrics::{initialize_prometheus_metrics, METRICS_ENABLED};
use angstrom_types::contract_bindings::angstrom::Angstrom::PoolKey;
use consensus::AngstromValidator;
use enr::k256::ecdsa::SigningKey;
use eyre::Context;
use reth_network_peers::pk2id;
use secp256k1::{Secp256k1, SecretKey};
use serde::Deserialize;
use testing_tools::types::config::TestnetConfig;

#[derive(Debug, Clone, Default, clap::Parser)]
pub struct TestnetCli {
    #[clap(long)]
    pub mev_guard:        bool,
    /// the amount of testnet nodes that will be spawned and connected to.
    #[clap(short, long, default_value = "5")]
    pub nodes_in_network: u64,
    /// eth rpc/ipc fork url
    #[clap(short, long)]
    pub eth_fork_url:     String,
    /// path to the toml file with the pool keys
    #[clap(short, long, default_value = "./bin/testnet/pool_key_config.toml")]
    pub pool_key_config:  PathBuf
}

impl TestnetCli {
    pub(crate) fn make_config(&self) -> eyre::Result<TestnetConfig> {
        let pool_keys = AllPoolKeyInners::load_pool_keys(&self.pool_key_config)?;

        Ok(TestnetConfig::new(self.nodes_in_network, pool_keys, &self.eth_fork_url, self.mev_guard))
    }
}

#[derive(Debug, Clone, Deserialize)]
struct AllPoolKeyInners {
    pool_keys: Option<Vec<PoolKeyInner>>
}

impl AllPoolKeyInners {
    fn load_pool_keys(config_path: &PathBuf) -> eyre::Result<Vec<PoolKey>> {
        if !config_path.exists() {
            return Err(eyre::eyre!("pool key config file does not exist at {:?}", config_path))
        }

        let toml_content = std::fs::read_to_string(config_path)
            .wrap_err_with(|| format!("could not read pool key config file {:?}", config_path))?;

        let node_config: Self = toml::from_str(&toml_content).wrap_err_with(|| {
            format!("could not deserialize pool key config file {:?}", config_path)
        })?;

        node_config.try_into()
    }
}

impl TryInto<Vec<PoolKey>> for AllPoolKeyInners {
    type Error = eyre::ErrReport;

    fn try_into(self) -> Result<Vec<PoolKey>, Self::Error> {
        let Some(keys) = self.pool_keys else { return Ok(Vec::new()) };

        keys.into_iter()
            .map(|key| {
                Ok::<_, eyre::ErrReport>(PoolKey {
                    currency0:   key.currency0.parse()?,
                    currency1:   key.currency1.parse()?,
                    fee:         U24::from(key.fee),
                    tickSpacing: I24::try_from(key.tick_spacing)?,
                    hooks:       key.hooks.parse()?
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone, Deserialize)]
struct PoolKeyInner {
    currency0:    String,
    currency1:    String,
    fee:          u64,
    tick_spacing: i64,
    hooks:        String
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, str::FromStr};

    use crate::cli::testnet::AllPoolKeyInners;

    #[test]
    fn test_read_config() {
        let path = PathBuf::from_str("./pool_key_config.toml").unwrap();
        println!("{:?}", path);

        let config = AllPoolKeyInners::load_pool_keys(&path);
        config.as_ref().unwrap();
        assert!(config.is_ok());
    }
}
