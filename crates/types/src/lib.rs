#![allow(macro_expanded_macro_exports_accessed_by_absolute_paths)]
// #![feature(more_maybe_bounds)]

pub mod consensus;
pub mod contract_bindings;
pub mod contract_payloads;
pub mod matching;
pub mod orders;
pub mod primitive;
pub mod reth_db_wrapper;
pub mod sol_bindings;

// #[cfg(feature = "testnet")]
// pub use sol_bindings::rpc_orders::{
//     random_ExactFlashOrder, random_ExactFlashOrder,
// random_ExactStandingOrder,     random_PartialFlashOrder,
// random_PartialStandingOrder, random_TopOfBlockOrder };
