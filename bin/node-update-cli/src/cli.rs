use std::path::PathBuf;

use alloy::providers::{Provider, ProviderBuilder};
use angstrom_types::primitive::{AngstromSigner, CHAIN_ID, KeyConfig, init_with_chain_id};
use clap::Parser;
use exe_runners::TaskExecutor;
use reth::chainspec::NamedChain;
use tracing::Level;
use tracing_subscriber::{filter, layer::SubscriberExt, util::SubscriberInitExt};
use url::Url;

use crate::commands::modify_fees::ModifyPoolFeesCommand;

#[derive(Debug, Clone, clap::Parser)]
pub struct NodeUpdaterCli {
    /// angstrom endpoint
    #[clap(short, long, default_value = "ws://localhost:8546", global = true)]
    pub node_endpoint: String,
    /// mainnet or sepolia ONLY
    #[clap(long, default_value = "mainnet")]
    pub chain:         NamedChain,
    #[clap(subcommand)]
    pub command:       NodeUpdateCommand
}

impl NodeUpdaterCli {
    pub async fn run(self, task_executor: TaskExecutor) -> eyre::Result<()> {
        let this = Self::parse();

        assert!(this.chain == NamedChain::Mainnet || this.chain == NamedChain::Sepolia);
        init_with_chain_id(this.chain as u64);

        init_tracing();

        let provider = ProviderBuilder::new().connect(&self.node_endpoint).await?;

        match self.command {
            NodeUpdateCommand::ModifyFees(modify_pool_fees_command) => {
                modify_pool_fees_command.run(provider).await?
            }
        };

        Ok(())
    }
}

#[derive(Debug, Clone, clap::Subcommand)]
pub enum NodeUpdateCommand {
    #[command(name = "modify-fees")]
    ModifyFees(ModifyPoolFeesCommand)
}

pub(crate) fn init_tracing() {
    let level = Level::INFO;

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
            .with_target("node_update_cli", level)
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
