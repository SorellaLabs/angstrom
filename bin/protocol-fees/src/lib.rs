use alloy_provider::{ProviderBuilder, WsConnect};
use angstrom_types_primitives::init_with_chain_id;
use clap::Parser;

pub mod cli;
mod client;
mod live;
pub mod types;
pub use client::*;

use crate::cli::{ProtocolFeesCli, init_tracing};

pub async fn run() -> eyre::Result<()> {
    init_tracing(3);
    init_with_chain_id(1);
    let cli = ProtocolFeesCli::parse();
    let provider = ProviderBuilder::new()
        .connect_ws(WsConnect::new(&cli.eth_ws_url))
        .await?;
    let client = ProtocolFeeFetcher::new(provider).await?;
    let collection = client.collectable_fees().await?;
    println!("{}", collection.calldata);
    println!("Collectible at block {} ({}):", collection.block.number, collection.block.hash);
    for fee in collection.fees {
        println!("{} {} ({})", fee.amount, fee.symbol, fee.asset);
    }
    println!("Bundle-held protocol fees: 0 authorized");
    Ok(())
}
