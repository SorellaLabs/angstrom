//! Pool Manager
//!
//! This crate provides pool management functionality for Angstrom, including:
//! - Order pool management for the network layer
//! - Type state pattern implementation for consensus vs rollup modes
//! - Mode-specific behavior through trait abstraction

use angstrom_network::NetworkHandle;
use angstrom_types::{block_sync::BlockSyncConsumer, sol_bindings::grouped_orders::AllOrders};
use validation::order::OrderValidatorHandle;

pub mod cache;
pub mod consensus;
pub mod order;
pub mod rollup;

/// Trait defining mode-specific behavior for PoolManager
///
/// This trait allows different operational modes (Consensus, Rollup) to
/// customize specific aspects of pool management behavior while sharing the
/// bulk of the implementation.
pub trait PoolManagerMode: Send + Sync + Unpin + 'static {
    /// Whether this mode requires networking functionality
    const REQUIRES_NETWORKING: bool;

    /// Mode-specific logic for processing/filtering orders for a proposal.
    ///
    /// Different modes may have different requirements for which orders should
    /// be included in proposals (e.g., consensus mode might filter based on
    /// consensus state).
    fn get_proposable_orders<V, GS, NH>(
        pool: &mut order::PoolManager<V, GS, NH, Self>
    ) -> Vec<AllOrders>
    where
        V: OrderValidatorHandle<Order = AllOrders> + Unpin,
        GS: BlockSyncConsumer,
        NH: NetworkHandle,
        Self: Sized;

    /// Poll mode-specific streams and handle mode-specific events.
    ///
    /// This method allows each mode to handle its own polling logic and event
    /// processing, keeping the main Future implementation cleaner and more
    /// maintainable.
    fn poll_mode_specific<V, GS, NH>(
        pool: &mut order::PoolManager<V, GS, NH, Self>,
        cx: &mut std::task::Context<'_>
    ) where
        V: OrderValidatorHandle<Order = AllOrders> + Unpin,
        GS: BlockSyncConsumer,
        NH: NetworkHandle,
        Self: Sized
    {
        // Default implementation does nothing - modes can override as needed
        let _ = (pool, cx);
    }
}

// Re-export main types and modes for convenience - following QuoterManager
// pattern
pub use consensus::{ConsensusMode, ConsensusPoolManager};
pub use order::*;
pub use rollup::{NoNetwork, RollupMode, RollupPoolManager};
