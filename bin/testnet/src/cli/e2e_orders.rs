use std::ops::Range;

use clap::Parser;
use testing_tools::types::config::DevnetConfig;

use super::testnet::TestnetCli;

#[derive(Parser, Clone, Debug)]
pub struct End2EndOrdersCli {
    // /// the amount of book orders per pool to generate. 5..10 is 5 to 10 orders
    // /// per pool
    // #[clap(short, long)]
    // pub order_amount_range: Range<usize>,
    // /// the range percent in which partial orders will generate, eg 0.05..0.1 is
    // /// 5 - 10% of orders will be partial orders
    // #[clap(short, long)]
    // pub partial_pct_range:  Range<f64>,
    #[clap(flatten)]
    pub testnet_config: TestnetCli
}
