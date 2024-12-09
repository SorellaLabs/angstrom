pub mod devnet;
pub mod e2e_orders;
pub mod testnet;
use angstrom_metrics::{initialize_prometheus_metrics, METRICS_ENABLED};
use clap::{ArgAction, Parser, Subcommand};
use devnet::DevnetCli;
use e2e_orders::End2EndOrdersCli;
use reth_tasks::TaskExecutor;
use testing_tools::types::config::{DevnetConfig, TestnetConfig};
use testnet::TestnetCli;
use tracing::Level;
use tracing_subscriber::{
    layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer, Registry
};

use crate::{run_devnet, run_testnet, simulations::e2e_orders::run_e2e_orders};

#[derive(Parser)]
pub struct AngstromTestnetCli {
    /// testnet or devnet commands
    #[clap(subcommand)]
    pub command:      TestnetSubcommmand,
    /// Set the minimum log level.
    ///
    /// -v      Errors
    /// -vv     Warnings
    /// -vvv    Info
    /// -vvvv   Debug
    /// -vvvvv  Traces
    #[clap(short = 'v', long, action = ArgAction::Count, default_value_t = 3, help_heading = "Display", global = true)]
    pub verbosity:    u8,
    /// enables the metrics
    #[clap(long, default_value = "false", global = true)]
    pub metrics:      bool,
    /// spawns the prometheus metrics exporter at the specified port
    /// Default: 6969
    #[clap(long, default_value = "6969", global = true)]
    pub metrics_port: u16
}

impl AngstromTestnetCli {
    pub async fn run_all(executor: TaskExecutor) -> eyre::Result<()> {
        let this = Self::parse();
        this.init_tracing();

        if this.metrics
            && initialize_prometheus_metrics(this.metrics_port)
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
        this.command.run_command(executor).await
    }

    fn init_tracing(&self) {
        let level = match self.verbosity - 1 {
            0 => Level::ERROR,
            1 => Level::WARN,
            2 => Level::INFO,
            3 => Level::DEBUG,
            _ => Level::TRACE
        };

        let layers = vec![
            layer_builder(format!("testnet={level}")),
            layer_builder(format!("devnet={level}")),
            layer_builder(format!("angstrom_rpc={level}")),
            layer_builder(format!("angstrom={level}")),
            layer_builder(format!("testing_tools={level}")),
            layer_builder(format!("matching_engine={level}")),
            layer_builder(format!("uniswap_v4={level}")),
            layer_builder(format!("consensus={level}")),
            layer_builder(format!("validation={level}")),
            layer_builder(format!("order_pool={level}")),
        ];

        tracing_subscriber::registry().with(layers).init();
    }
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

#[derive(Debug, Subcommand, Clone)]
pub enum TestnetSubcommmand {
    #[command(name = "testnet")]
    Testnet(TestnetCli),
    #[command(name = "devnet")]
    Devnet(DevnetCli),
    #[command(name = "e2e")]
    End2EndOrders(End2EndOrdersCli)
}

impl TestnetSubcommmand {
    async fn run_command(self, executor: TaskExecutor) -> eyre::Result<()> {
        match self {
            TestnetSubcommmand::Testnet(testnet_cli) => run_testnet(executor, testnet_cli).await,
            TestnetSubcommmand::Devnet(devnet_cli) => run_devnet(executor, devnet_cli).await,
            TestnetSubcommmand::End2EndOrders(e2e_cli) => run_e2e_orders(executor, e2e_cli).await
        }
    }
}
