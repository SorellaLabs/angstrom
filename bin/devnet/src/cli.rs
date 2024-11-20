use clap::{ArgAction, Parser};
use testing_tools::controllers::devnet::DevnetConfig;
use tracing::Level;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

#[derive(Parser)]
pub struct Cli {
    /// starting port for the rpc for submitting transactions.
    /// each node will have an rpc submission endpoint at this port + their
    /// node's number
    /// i.e. node 3/3 will have port 4202 if this value is set to 4200
    #[clap(short = 'p', long, default_value_t = 42000)]
    pub starting_port:           u16,
    /// the speed in which anvil will mine blocks.
    #[clap(short, long, default_value = "12")]
    pub testnet_block_time_secs: u64,
    /// the amount of testnet nodes that will be spawned and connected to.
    /// this will change in the future but is good enough for testing currently
    #[clap(short, long, default_value = "2")]
    pub nodes_in_network:        u64,
    /// the secret key/address to use as the controller
    #[clap(short, long, default_value = "7")]
    pub anvil_key:               u16,
    /// starting block to fork
    #[clap(short = 's', long)]
    pub fork_block:              Option<u64>,
    /// fork url
    #[clap(long, requires = "fork_block")]
    pub fork_url:                Option<String>,
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
    pub fn build_config() -> DevnetConfig {
        let this = Self::parse();
        this.init_tracing();

        DevnetConfig::new(
            this.anvil_key as usize,
            this.nodes_in_network,
            this.starting_port,
            this.fork_block,
            this.fork_url
        )
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
            // .with_default_directive(format!("testnet={level}").parse().unwrap())
            .from_env_lossy();
        // .add_directive(format!("angstrom={level}").parse().unwrap())
        // .add_directive(format!("testing_tools={level}").parse().unwrap());

        let layer = tracing_subscriber::fmt::layer()
            .with_ansi(true)
            .with_target(true)
            .with_filter(filter)
            .boxed();

        tracing_subscriber::registry().with(vec![layer]).init();
    }
}
