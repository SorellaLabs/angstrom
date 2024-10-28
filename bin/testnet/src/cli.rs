use clap::{ArgAction, Parser};
use testing_tools::testnet_controllers::{AngstromTestnetConfig, TestnetKind};
use tracing::Level;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

#[derive(Parser)]
pub struct Cli {
    /// starting port for the rpc for submitting transactions.
    /// each node will have an rpc submission endpoint at this port + their
    /// node's number
    /// i.e. node 3/3 will have port 4202 if this value is set to 4200
    #[clap(short = 'p', long, default_value_t = 4200)]
    pub starting_port:           u16,
    /// the speed in which anvil will mine blocks.
    #[clap(short, long, default_value = "12")]
    pub testnet_block_time_secs: u64,
    /// the amount of testnet nodes that will be spawned and connected to.
    /// this will change in the future but is good enough for testing currently
    #[clap(short, long, default_value = "2")]
    pub nodes_in_network:        u64,
    /// Set the minimum log level.
    ///
    /// -v      Errors
    /// -vv     Warnings
    /// -vvv    Info
    /// -vvvv   Debug
    /// -vvvvv  Traces
    #[clap(short = 'v', long, action = ArgAction::Count, default_value_t = 3, help_heading = "Display")]
    pub verbosity:               u8
}

impl Cli {
    pub fn build_config() -> AngstromTestnetConfig {
        let this = Self::parse();
        this.init_tracing();

        AngstromTestnetConfig {
            intial_node_count:       this.nodes_in_network,
            initial_rpc_port:        this.starting_port,
            testnet_block_time_secs: this.testnet_block_time_secs,
            testnet_kind:            TestnetKind::new_raw()
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

        let filter = EnvFilter::builder()
            .with_default_directive(format!("testnet={level}").parse().unwrap())
            .from_env_lossy()
            .add_directive(format!("angstrom={level}").parse().unwrap())
            .add_directive(format!("testing_tools={level}").parse().unwrap());

        let layer = tracing_subscriber::fmt::layer()
            .with_ansi(true)
            .with_target(true)
            .with_filter(filter)
            .boxed();

        tracing_subscriber::registry().with(vec![layer]).init();
    }
}
