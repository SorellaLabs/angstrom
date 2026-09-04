use alloy_provider::{ProviderBuilder, WsConnect};
use angstrom_types::init_with_chain_id;
use clap::Parser;
mod client;
pub mod types;
pub use client::*;

use crate::cli::{ProtocolFeesCli, init_tracing};

pub mod cli;

pub async fn run() -> eyre::Result<()> {
    init_tracing(3);
    init_with_chain_id(1);

    let cli = ProtocolFeesCli::parse();

    let eth_provider = ProviderBuilder::new()
        .connect_ws(WsConnect::new(&cli.eth_ws_url))
        .await?;

    let client = ProtocolFeeFetcher::new(eth_provider).await?;

    let starting_point = client.get_last_fee_pull().await?;

    Ok(())
}
