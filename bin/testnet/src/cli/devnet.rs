use clap::Parser;
use testing_tools::types::config::DevnetConfig;

#[derive(Parser, Clone, Debug)]
pub struct DevnetCli {
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
    #[clap(short, long, default_value = "3")]
    pub nodes_in_network:        u64,
    /// the secret key/address to use as the controller
    #[clap(short, long, default_value = "7")]
    pub anvil_key:               u16,
    /// starting block to fork
    #[clap(short = 's', long)]
    pub fork_block:              Option<u64>,
    /// fork url
    #[clap(long, requires = "fork_block")]
    pub fork_url:                Option<String>
}

impl DevnetCli {
    pub fn make_config(self) -> DevnetConfig {
        DevnetConfig::new(self.nodes_in_network, self.starting_port, self.fork_block, self.fork_url)
    }
}
