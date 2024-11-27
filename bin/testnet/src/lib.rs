//! CLI definition and entrypoint to executable
pub mod cli;
pub(crate) mod utils;

use clap::Parser;
use reth::{tasks::TaskExecutor, CliRunner};
use reth_provider::test_utils::NoopProvider;
use testing_tools::{controllers::enviroments::AngstromTestnet, types::config::TestnetConfig};
use tracing::Level;
use tracing_subscriber::{
    layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer, Registry
};

use crate::cli::testnet::TestnetCli;

pub fn run() -> eyre::Result<()> {
    CliRunner::default().run_command_until_exit(|ctx| execute(ctx.task_executor))
}

async fn execute(executor: TaskExecutor) -> eyre::Result<()> {
    let cli = TestnetCli::parse();

    let level = Level::DEBUG;
    let layers = vec![
        layer_builder(format!("devnet={level}")),
        layer_builder(format!("testnet={level}")),
        layer_builder(format!("angstrom={level}")),
        layer_builder(format!("testing_tools={level}")),
        layer_builder(format!("uniswap_v4={level}")),
        layer_builder(format!("validation={level}")),
    ];

    tracing_subscriber::registry().with(layers).init();

    executor.spawn_critical("metrics", cli.clone().init_metrics());

    let testnet_config = cli.load_config()?;
    let _my_node_config = testnet_config.my_node_config()?;
    let config = TestnetConfig::new(3, Vec::new(), "ws://35.245.117.24:8546");

    let testnet = AngstromTestnet::spawn_testnet(NoopProvider::default(), config).await?;

    executor.spawn_critical("testnet", testnet.run_testnet());

    Ok(())
}

fn layer_builder(filter_str: String) -> Box<dyn Layer<Registry> + Send + Sync> {
    let filter = EnvFilter::builder()
        .with_default_directive(filter_str.parse().unwrap())
        .from_env_lossy();

    tracing_subscriber::fmt::layer()
        .with_ansi(true)
        .with_target(true)
        .with_filter(filter)
        .boxed()
}
