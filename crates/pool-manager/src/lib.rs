//! Pool Manager
//!
//! This crate provides pool management functionality for Angstrom, including:
//! - Order pool management for the network layer
//! - Type state pattern implementation for consensus vs rollup modes

use std::task::Context;

use angstrom_network::{NetworkHandle, StromNetworkEvent};
use angstrom_types::{block_sync::BlockSyncConsumer, sol_bindings::grouped_orders::AllOrders};
use tokio_stream::wrappers::UnboundedReceiverStream;
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
    fn get_proposable_orders<V, GS, NH>(pool: &mut order::PoolManager<V, GS, NH, Self>) -> Vec<AllOrders>
    where
        V: OrderValidatorHandle<Order = AllOrders> + Unpin,
        GS: BlockSyncConsumer,
        NH: NetworkHandle,
        Self: Sized;

    /// Hook for any mode-specific polling logic within the main future's poll
    /// loop.
    ///
    /// This allows modes to add their own polling behavior (e.g., consensus
    /// streams, mode-specific timers, etc.) without duplicating the entire
    /// Future implementation.
    fn poll_mode_specific<V, GS, NH>(
        _pool: &mut order::PoolManager<V, GS, NH, Self>,
        _cx: &mut Context<'_>
    ) where
        V: OrderValidatorHandle<Order = AllOrders> + Unpin,
        GS: BlockSyncConsumer,
        NH: NetworkHandle<Events<'static> = UnboundedReceiverStream<StromNetworkEvent>> + 'static,
        Self: Sized
    {
        // Default to no-op - modes can override if they need specific polling
        // behavior
    }
}

// Re-export order pool management types
// Re-export mode types for convenience
pub use consensus::{ConsensusMode, ConsensusPoolManager};
pub use order::*;
pub use rollup::{RollupMode, RollupPoolManager};
