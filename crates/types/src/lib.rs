#![allow(clippy::too_long_first_doc_paragraph)]
#![allow(macro_expanded_macro_exports_accessed_by_absolute_paths)]

pub mod block_sync;
pub mod consensus;
pub mod contract_bindings;
pub mod contract_payloads;
pub mod matching;
pub mod mev_boost;
pub mod orders;
pub mod pair_with_price;
pub mod primitive;
pub mod reth_db_provider;
pub mod reth_db_wrapper;
pub mod sol_bindings;
pub mod testnet;
pub mod uni_structure;

// the 2nd cfg condition is for the GH CI
#[cfg(any(
    all(not(feature = "testnet"), not(feature = "testnet-sepolia")),
    all(feature = "testnet", feature = "testnet-sepolia")
))]
pub const CHAIN_ID: u64 = 1;

#[cfg(all(feature = "testnet", not(feature = "testnet-sepolia")))]
pub const CHAIN_ID: u64 = 1;

#[cfg(all(feature = "testnet-sepolia", not(feature = "testnet")))]
pub const CHAIN_ID: u64 = 11155111;
