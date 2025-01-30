// Tests around building the Angstrom bundle

use std::collections::HashMap;

use alloy::primitives::{Address, FixedBytes};
use angstrom_types::{
    contract_payloads::angstrom::AngstromBundle, matching::uniswap::PoolSnapshot
};
use tracing::Level;

pub fn with_tracing<T>(f: impl FnOnce() -> T) -> T {
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(Level::TRACE)
        .with_line_number(true)
        .with_file(true)
        .finish();
    tracing::subscriber::with_default(subscriber, f)
}

#[test]
fn build_bundle() {
    with_tracing(|| {
        let pools: HashMap<FixedBytes<32>, (Address, Address, PoolSnapshot, u16)> = HashMap::new();
        AngstromBundle::for_gas_finalization(vec![], vec![], &pools);
    })
}
