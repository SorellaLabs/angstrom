use std::collections::HashMap;

use alloy::primitives::FixedBytes;
use angstrom_metrics::FinalizationOrderPoolMetricsWrapper;
use angstrom_types::sol_bindings::grouped_orders::{AllOrders, OrderWithStorageData};
use angstrom_utils::map::OwnedMap;

pub struct FinalizationPool {
    id_to_orders: HashMap<FixedBytes<32>, AllOrders>,
    block_to_ids: HashMap<u64, Vec<FixedBytes<32>>>,
    metrics:      FinalizationOrderPoolMetricsWrapper
}

impl Default for FinalizationPool {
    fn default() -> Self {
        Self::new()
    }
}

impl FinalizationPool {
    pub fn new() -> Self {
        Self {
            block_to_ids: HashMap::default(),
            id_to_orders: HashMap::default(),
            metrics:      FinalizationOrderPoolMetricsWrapper::new()
        }
    }

    pub fn new_orders(&mut self, block: u64, orders: Vec<OrderWithStorageData<AllOrders>>) {
        let ids = orders
            .into_iter()
            .map(|order| {
                let id = order.order_hash();
                self.id_to_orders.insert(id, order.order);

                self.metrics.incr_total_orders();

                id
            })
            .collect::<Vec<_>>();

        assert!(self.block_to_ids.insert(block, ids).is_none());

        self.metrics.incr_blocks_tracked();
    }

    pub fn has_order(&mut self, order: &FixedBytes<32>) -> bool {
        self.id_to_orders.contains_key(order)
    }

    pub fn reorg(&mut self, orders: Vec<FixedBytes<32>>) -> impl Iterator<Item = AllOrders> + '_ {
        orders.into_iter().filter_map(|id| {
            self.id_to_orders
                .remove(&id)
                .owned_map(|| self.metrics.decr_total_orders())
        })
    }

    pub fn finalized(&mut self, block: u64) -> Vec<AllOrders> {
        self.block_to_ids
            .remove(&block)
            .map(|ids| {
                ids.into_iter()
                    .filter_map(|hash| {
                        self.id_to_orders
                            .remove(&hash)
                            .owned_map(|| self.metrics.decr_total_orders())
                    })
                    .collect()
            })
            .owned_map(|| self.metrics.decr_blocks_tracked())
            .unwrap_or_default()
    }
}
