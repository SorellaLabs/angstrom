//! Pool Manager
//!
//! This crate provides pool management functionality for Angstrom, including:
//! - Order pool management for the network layer
//! - Type state pattern implementation for consensus vs rollup modes

pub mod cache;
pub mod consensus;
pub mod order;
pub mod rollup;

// Re-export order pool management types
// Re-export mode types for convenience
pub use consensus::{ConsensusMode, ConsensusPoolManager};
pub use order::*;
pub use rollup::{RollupMode, RollupPoolManager};
