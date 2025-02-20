use std::{
    net::IpAddr,
    path::{Path, PathBuf},
    str::FromStr
};

use alloy::signers::local::PrivateKeySigner;
use alloy_primitives::{
    aliases::{I24, U24},
    Address, Bytes, U160
};
use alloy_signer_local::LocalSigner;
use angstrom_metrics::{initialize_prometheus_metrics, METRICS_ENABLED};
use angstrom_types::{contract_bindings::angstrom::Angstrom::PoolKey, matching::SqrtPriceX96};
use consensus::AngstromValidator;
use enr::k256::ecdsa::SigningKey;
use eyre::Context;
use reth_network_peers::pk2id;
use secp256k1::{Secp256k1, SecretKey};
use serde::Deserialize;
use testing_tools::{
    types::{
        config::TestnetConfig,
        initial_state::{Erc20ToDeploy, InitialStateConfig, PartialConfigPoolKey}
    },
    utils::workspace_dir
};

#[derive(Debug, Clone, clap::Parser)]
pub struct TestnetCli {
    #[clap(long)]
    pub mev_guard:              bool,
    #[clap(short, long)]
    pub leader_eth_rpc_port:    Option<u16>,
    #[clap(short, long)]
    pub angstrom_base_rpc_port: Option<u16>,
    /// the amount of testnet nodes that will be spawned and connected to.
    #[clap(short, long, default_value = "3")]
    pub nodes_in_network:       u64,
    /// eth rpc/ipc fork url
    #[clap(short, long, default_value = "ws://localhost:8546")]
    pub eth_fork_url:           String,
    /// path to the toml file with the pool keys
    #[clap(short, long, default_value = "./bin/testnet/testnet-config.toml")]
    pub pool_key_config:        PathBuf
}

impl TestnetCli {
    pub(crate) fn make_config(&self) -> eyre::Result<TestnetConfig> {
        let initial_state_config = AllPoolKeyInners::load_toml_config(&self.pool_key_config)?;

        Ok(TestnetConfig::new(
            self.nodes_in_network,
            &self.eth_fork_url,
            self.mev_guard,
            self.leader_eth_rpc_port,
            self.angstrom_base_rpc_port,
            initial_state_config
        ))
    }
}

impl Default for TestnetCli {
    fn default() -> Self {
        let mut workspace_dir = workspace_dir();
        workspace_dir.push("bin/testnet/pool_key_config.toml");
        Self {
            mev_guard:              false,
            leader_eth_rpc_port:    None,
            angstrom_base_rpc_port: None,
            nodes_in_network:       3,
            eth_fork_url:           "ws://localhost:8546".to_string(),
            pool_key_config:        workspace_dir
        }
    }
}

impl TryInto<InitialStateConfig> for AllPoolKeyInners {
    type Error = eyre::ErrReport;

    fn try_into(self) -> Result<InitialStateConfig, Self::Error> {
        Ok(InitialStateConfig {
            addresses_with_tokens: self
                .addresses_with_tokens
                .iter()
                .map(|addr| Address::from_str(addr))
                .collect::<Result<Vec<_>, _>>()?,
            tokens_to_deploy:      self
                .tokens_to_deploy
                .clone()
                .iter()
                .map(|val| val.clone().try_into())
                .collect::<Result<Vec<_>, _>>()?,
            pool_keys:             self.try_into()?
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
struct AllPoolKeyInners {
    addresses_with_tokens: Vec<String>,
    tokens_to_deploy:      Vec<TokenToDeploy>,
    pool_keys:             Option<Vec<PoolKeyInner>>
}

impl AllPoolKeyInners {
    fn load_toml_config(config_path: &PathBuf) -> eyre::Result<InitialStateConfig> {
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

impl TryInto<Vec<PartialConfigPoolKey>> for AllPoolKeyInners {
    type Error = eyre::ErrReport;

    fn try_into(self) -> Result<Vec<PartialConfigPoolKey>, Self::Error> {
        let Some(keys) = self.pool_keys else { return Ok(Vec::new()) };

        keys.into_iter()
            .map(|key| {
                Ok::<_, eyre::ErrReport>(PartialConfigPoolKey::new(
                    key.fee,
                    key.tick_spacing,
                    key.liquidity.parse()?,
                    SqrtPriceX96::at_tick(key.tick)?
                ))
            })
            .collect()
    }
}

#[derive(Debug, Clone, Deserialize)]
struct PoolKeyInner {
    // currency0:    String,
    // currency1:    String,
    fee:          u64,
    tick_spacing: i32,
    liquidity:    String,
    tick:         i32
}

#[derive(Debug, Clone, Deserialize)]
struct TokenToDeploy {
    name:    String,
    symbol:  String,
    address: Option<String>
}

impl TryInto<Erc20ToDeploy> for TokenToDeploy {
    type Error = eyre::ErrReport;

    fn try_into(self) -> Result<Erc20ToDeploy, Self::Error> {
        Ok(Erc20ToDeploy::new(
            &self.name,
            &self.symbol,
            self.address
                .map(|addr| Address::from_str(&addr))
                .transpose()?
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, str::FromStr};

    use crate::cli::testnet::AllPoolKeyInners;

    #[test]
    fn test_read_config() {
        let path = PathBuf::from_str("./testnet-config.toml").unwrap();
        println!("{:?}", path);

        let config = AllPoolKeyInners::load_toml_config(&path);
        config.as_ref().unwrap();
        assert!(config.is_ok());
    }
}
