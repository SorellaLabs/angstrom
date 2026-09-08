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
    let calculation = ProtocolFeeFetcher::new(provider).await?.calculate().await?;
    let ledger = calculation.ledger()?;

    println!("Bundle-held ledger over {} block(s) with activity:", calculation.blocks.len());
    for row in &ledger {
        let show = |amount| match calculation.token(row.asset) {
            Some(token) => token.format(amount),
            None => Ok(format!("{amount} raw (unresolved token)"))
        };
        println!(
            "{}: saved gross {}, pulled {}, candidate outstanding {}",
            row.asset,
            show(row.saved_gross)?,
            show(row.pulled_against_saved)?,
            show(row.candidate_outstanding_saved)?
        );
    }
    println!("Collectible bundle-held fees: 0");
    println!(
        "No collection calldata for {}: the current audit does not authorize bundle-held \
         withdrawals. Candidate outstanding saved is a collective commitment remainder; it does \
         not establish protocol ownership or satisfy the protected-balance and reservation checks.",
        cli.recipient
    );
    Ok(())
}
