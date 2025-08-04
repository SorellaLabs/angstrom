//! Pool Manager
//!
//! This crate provides pool management functionality for Angstrom, including:
//! - Order pool management for the network layer
//! - Type state pattern implementation for consensus vs rollup modes
//! - Separate struct types for each operational mode

pub mod cache;
pub mod common;
pub mod consensus;
pub mod order;
pub mod rollup;

// Re-export main types for convenience
pub use consensus::ConsensusPoolManager;
pub use order::*;
pub use rollup::RollupPoolManager;
