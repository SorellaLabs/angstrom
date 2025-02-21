use clap::Parser;

use super::testnet::TestnetCli;

#[derive(Debug, Clone, Parser)]
pub struct CompareEnginesCli {
    #[arg(short = 'z', default_value_t = false)]
    pub include_amm:    bool,
    #[clap(flatten)]
    pub testnet_config: TestnetCli
}
