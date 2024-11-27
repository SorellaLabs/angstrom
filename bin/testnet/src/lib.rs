//! CLI definition and entrypoint to executable
pub mod cli;
pub(crate) mod utils;

use clap::Parser;
use reth::{tasks::TaskExecutor, CliRunner};
use reth_provider::test_utils::NoopProvider;
use testing_tools::{controllers::enviroments::AngstromTestnet, types::config::TestnetConfig};

use crate::cli::testnet::TestnetCli;

pub fn run() -> eyre::Result<()> {
    CliRunner::default().run_command_until_exit(|ctx| execute(ctx.task_executor))
}

async fn execute(executor: TaskExecutor) -> eyre::Result<()> {
    let cli = TestnetCli::parse();
    executor.spawn_critical("metrics", cli.clone().init_metrics());

    let testnet_config = cli.load_config()?;
    let _my_node_config = testnet_config.my_node_config()?;
    let config = TestnetConfig::new(3, Vec::new(), "ws://35.245.117.24:8546");

    let testnet = AngstromTestnet::spawn_testnet(NoopProvider::default(), config).await?;

    executor.spawn_critical("testnet", testnet.run_testnet());

    Ok(())
}
