pub mod testnet;

use clap::{ArgAction, Parser, Subcommand};
use testing_tools::types::config::DevnetConfig;
use tracing::Level;
use tracing_subscriber::{
    layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer, Registry
};

#[derive(Parser)]
pub struct Cli {
    /// testnet or devnet commands
    #[clap(subcommand)]
    pub command:   TestnetSubcommmand,
    /// Set the minimum log level.
    ///
    /// -v      Errors
    /// -vv     Warnings
    /// -vvv    Info
    /// -vvvv   Debug
    /// -vvvvv  Traces
    #[clap(short = 'v', long, action = ArgAction::Count, default_value_t = 3, help_heading = "Display", global = true)]
    pub verbosity: u8
}

impl Cli {
    pub fn build_devnet_config() -> DevnetConfig {
        let this = Self::parse();
        this.init_tracing();

        match this.command {
            TestnetSubcommmand::Devnet => todo!(),
            _ => panic!("cannot build devnet config for the testnet")
        }
    }

    pub fn build_testnet_config() -> DevnetConfig {
        let this = Self::parse();
        this.init_tracing();

        match this.command {
            TestnetSubcommmand::Testnet => todo!(),
            _ => panic!("cannot build testnet config for the devnet")
        }
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
            layer_builder(format!("devnet={level}")),
            layer_builder(format!("angstrom={level}")),
            layer_builder(format!("testing_tools={level}")),
            layer_builder(format!("uniswap_v4={level}")),
            layer_builder(format!("validation={level}")),
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
    #[command(subcommand)]
    #[command(name = "testnet")]
    Testnet,
    #[command(subcommand)]
    #[command(name = "devnet")]
    Devnet
}
