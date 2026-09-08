use alloy_provider::{ProviderBuilder, WsConnect};
use angstrom_types_primitives::init_with_chain_id;
use clap::Parser;

pub mod cli;
mod client;
pub mod types;

use crate::{
    cli::{ProtocolFeesCli, init_tracing},
    client::ProtocolFeeFetcher
};

pub async fn run() -> eyre::Result<()> {
    init_tracing(3);
    init_with_chain_id(1);
    let cli = ProtocolFeesCli::parse();
    let provider = ProviderBuilder::new()
        .connect_ws(WsConnect::new(&cli.eth_ws_url))
        .await?;
    let fees = ProtocolFeeFetcher::new(provider).await?.calculate().await?;

    println!("Bundle savings at block {} ({}):", fees.block.number, fees.block.hash);
    for token in fees.tokens {
        println!(
            "{} ({}): gross saved {}, collectible 0",
            token.symbol, token.asset, token.saved_gross
        );
    }
    println!("Collectible bundle-held fees: 0");
    println!(
        "No collection calldata for {}: the current audit does not authorize bundle-held \
         withdrawals. Gross savings do not establish protocol ownership or satisfy the \
         protected-balance and reservation checks.",
        cli.recipient
    );
    Ok(())
}
