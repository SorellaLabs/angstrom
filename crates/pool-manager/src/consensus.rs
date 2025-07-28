use std::task::Context;

use angstrom_types::{
    block_sync::BlockSyncConsumer, network::NetworkHandle, sol_bindings::grouped_orders::AllOrders
};
use validation::order::OrderValidatorHandle;

use crate::order::{PoolManager, PoolManagerMode};

/// Consensus mode for PoolManager - includes consensus-specific state and
/// behavior
#[derive(Debug)]
pub struct ConsensusMode {
    /// Consensus-specific state could be added here in the future
    /// For example: consensus streams, pre-proposal tracking, etc.
    _consensus_state: ()
}

impl ConsensusMode {
    pub fn new() -> Self {
        Self { _consensus_state: () }
    }
}

impl Default for ConsensusMode {
    fn default() -> Self {
        Self::new()
    }
}

impl PoolManagerMode for ConsensusMode {
    fn get_proposable_orders<V, GS, NH>(pool: &mut PoolManager<V, GS, NH, Self>) -> Vec<AllOrders>
    where
        V: OrderValidatorHandle<Order = AllOrders> + Unpin,
        GS: BlockSyncConsumer,
        NH: NetworkHandle,
        Self: Sized
    {
        // In consensus mode, we might need to filter orders based on consensus state
        // or apply consensus-specific validation rules
        pool.order_indexer
            .get_all_orders_with_parked()
            .into_all_orders()
    }

    fn poll_mode_specific<V, GS, NH>(
        _pool: &mut PoolManager<V, GS, NH, Self>,
        _cx: &mut Context<'_>
    ) where
        V: OrderValidatorHandle<Order = AllOrders> + Unpin,
        GS: BlockSyncConsumer,
        NH: NetworkHandle,
        Self: Sized
    {
        // In the future, this could poll consensus streams, handle pre-proposal
        // events, etc. For now, consensus mode doesn't need additional
        // polling beyond the shared logic
    }
}

impl<V, GlobalSync, NH> PoolManager<V, GlobalSync, NH, ConsensusMode>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GlobalSync: BlockSyncConsumer,
    NH: NetworkHandle
{
    /// Consensus-specific order processing logic
    ///
    /// This method provides a convenient way to get proposable orders
    /// that respects the consensus mode's filtering logic.
    pub fn get_consensus_orders(&mut self) -> Vec<AllOrders> {
        ConsensusMode::get_proposable_orders(self)
    }

    /// Handle consensus-specific events
    ///
    /// This can be extended in the future to handle consensus streams,
    /// pre-proposal processing, consensus round transitions, etc.
    pub fn on_consensus_event(&mut self, _event: ()) {
        // Handle consensus-specific events here
        // This could include pre-proposal processing, consensus round
        // transitions, etc.
    }
}
