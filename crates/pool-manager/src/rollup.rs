use angstrom_types::{block_sync::BlockSyncConsumer, sol_bindings::grouped_orders::AllOrders, network::NetworkHandle};
use validation::order::OrderValidatorHandle;

use crate::order::{PoolManager, PoolManagerMode};

/// Rollup mode for PoolManager - simpler behavior without consensus logic
#[derive(Debug)]
pub struct RollupMode;

// RollupMode automatically derives Send + Sync since it has no fields

impl RollupMode {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RollupMode {
    fn default() -> Self {
        Self::new()
    }
}

impl PoolManagerMode for RollupMode {
    fn get_proposable_orders<V, GS, NH>(pool: &mut PoolManager<V, GS, NH, Self>) -> Vec<AllOrders>
    where
        V: OrderValidatorHandle<Order = AllOrders> + Unpin,
        GS: BlockSyncConsumer,
        NH: NetworkHandle,
        Self: Sized
    {
        // In rollup mode, we process all orders without consensus filtering
        // This is typically simpler and more direct
        pool.order_indexer
            .get_all_orders_with_parked()
            .into_all_orders()
    }

    // The default no-op for poll_mode_specific is sufficient for RollupMode
    // as it doesn't need any mode-specific polling behavior
}

impl<V, GlobalSync, NH> PoolManager<V, GlobalSync, NH, RollupMode>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GlobalSync: BlockSyncConsumer,
    NH: NetworkHandle,
{
    /// Rollup-specific order processing logic
    ///
    /// This method provides a convenient way to get proposable orders
    /// for rollup mode, which processes all orders without consensus filtering.
    pub fn get_rollup_orders(&mut self) -> Vec<AllOrders> {
        RollupMode::get_proposable_orders(self)
    }
}
