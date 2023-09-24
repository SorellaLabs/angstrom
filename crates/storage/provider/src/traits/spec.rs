use std::sync::Arc;

use reth_primitives::ChainSpec;

/// A trait for reading the current chainspec.
pub trait ChainSpecProvider: Send + Sync {
    /// Get an [`Arc`] to the chainspec.
    fn chain_spec(&self) -> Arc<ChainSpec>;
}
