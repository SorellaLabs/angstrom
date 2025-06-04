use std::{path::PathBuf, str::FromStr};

use alloy_primitives::Address;
use alloy_provider::ext::AnvilApi;
use angstrom_types::{matching::SqrtPriceX96, testnet::InitialTestnetState};
use eyre::Context;
use reth::tasks::TaskExecutor;
use reth_provider::BlockNumReader;
use serde::Deserialize;
use telemetry::blocklog::BlockLog;
use testing_tools::{
    providers::{AnvilInitializer, AnvilProvider, AnvilStateProvider, WalletProvider},
    types::{
        GlobalTestingConfig, WithWalletProvider,
        config::{ReplayConfig, TestingNodeConfig},
        initial_state::{Erc20ToDeploy, InitialStateConfig, PartialConfigPoolKey}
    }
};
use tracing::Level;
use tracing_subscriber::{filter, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, Clone, clap::Parser)]
pub struct ReplayCli {
    /// Is this a replay meant to be performed on a local testnet?
    #[clap(short, long, action)]
    pub testnet_replay:  bool,
    /// eth rpc/ipc fork url
    #[clap(short, long, default_value = "ws://localhost:8546")]
    pub eth_fork_url:    String,
    /// path to the toml file with the pool keys
    #[clap(short, long, default_value = "./bin/testnet/testnet-config.toml")]
    pub pool_key_config: PathBuf
}

impl ReplayCli {
    pub fn load_pool_keys(&self) -> eyre::Result<InitialStateConfig> {
        AllPoolKeyInners::load_toml_config(&self.pool_key_config)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct AllPoolKeyInners {
    addresses_with_tokens: Vec<String>,
    tokens_to_deploy:      Vec<TokenToDeploy>,
    pool_keys:             Option<Vec<PoolKeyInner>>
}

impl AllPoolKeyInners {
    pub(crate) fn load_toml_config(config_path: &PathBuf) -> eyre::Result<InitialStateConfig> {
        if !config_path.exists() {
            return Err(eyre::eyre!("pool key config file does not exist at {:?}", config_path));
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

pub async fn build_log_and_provider(
    node_config: TestingNodeConfig<ReplayConfig>,
    raw_log: BlockLog,
    ex: TaskExecutor
) -> eyre::Result<(BlockLog, AnvilProvider<WalletProvider>, Option<InitialTestnetState>)> {
    if node_config.global_config.testnet_replay() {
        tracing::info!("Starting testnet replay");
        let keys = node_config.global_config.initial_state_config().pool_keys;
        tracing::info!(?keys, "Pool keys");

        // Make our Initializer
        let (init, anvil, deployed) = AnvilInitializer::new(node_config.clone(), vec![]).await?;

        // Make our wrapped state provider
        let state_provider = AnvilStateProvider::new(init);
        let swp = state_provider.as_wallet_state_provider();

        // Make our wrapped provider
        let mut provider = AnvilProvider::new(state_provider, anvil, Some(deployed));

        // Setup our thread for doing things
        let handle = tokio::runtime::Handle::current().clone();
        std::thread::spawn(move || handle.block_on(swp.listen_to_new_blocks()));

        let new_init = provider.provider_mut().provider_mut();

        new_init.deploy_pool_fulls(keys).await?;
        let initial_state = new_init.initialize_state_no_bytes(ex).await?;
        new_init.rpc_provider().anvil_mine(Some(10), None).await?;

        let blocknum = provider.state_provider().last_block_number()?;
        Ok((raw_log.at_block(blocknum), provider.into_state_provider(), Some(initial_state)))
    } else {
        // Otherwise we want to just fork the current chain as specified on the command
        // line
        tracing::info!("Forking target network");
        let (wallet, instance) = node_config.spawn_anvil_rpc().await?;
        let provider = AnvilProvider::new(AnvilStateProvider::new(wallet), instance, None);
        Ok((raw_log, provider, None))
    }
}

pub fn init_tracing(verbosity: u8) {
    let level = match verbosity - 1 {
        0 => Level::ERROR,
        1 => Level::WARN,
        2 => Level::INFO,
        3 => Level::DEBUG,
        _ => Level::TRACE
    };

    let envfilter = filter::EnvFilter::builder().try_from_env().ok();
    let format = tracing_subscriber::fmt::layer()
        .with_ansi(true)
        .with_target(true);

    if let Some(f) = envfilter {
        let _ = tracing_subscriber::registry()
            .with(format)
            .with(f)
            .try_init();
    } else {
        let filter = filter::Targets::new()
            .with_target("testnet", level)
            .with_target("devnet", level)
            .with_target("angstrom_rpc", level)
            .with_target("angstrom", level)
            .with_target("testing_tools", level)
            .with_target("angstrom_eth", level)
            .with_target("matching_engine", level)
            .with_target("uniswap_v4", level)
            .with_target("consensus", level)
            .with_target("validation", level)
            .with_target("order_pool", level);
        let _ = tracing_subscriber::registry()
            .with(format)
            .with(filter)
            .try_init();
    }
}
