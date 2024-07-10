use std::collections::HashMap;

use alloy_primitives::FixedBytes;
use angstrom_types::sol_bindings::grouped_orders::{AllOrders, OrderWithStorageData};

pub struct FinalizationPool {
    id_to_orders: HashMap<FixedBytes<32>, AllOrders>,
    block_to_ids: HashMap<u64, Vec<FixedBytes<32>>>
}

impl Default for FinalizationPool {
    fn default() -> Self {
        Self::new()
    }
}

impl FinalizationPool {
    pub fn new() -> Self {
        Self { block_to_ids: HashMap::default(), id_to_orders: HashMap::default() }
    }

    pub fn new_orders(&mut self, block: u64, orders: Vec<OrderWithStorageData<AllOrders>>) {
        let ids = orders
            .into_iter()
            .map(|order| {
                let id = order.order_hash();
                self.id_to_orders.insert(id, order.order);

                id
            })
            .collect::<Vec<_>>();

        assert!(self.block_to_ids.insert(block, ids).is_none());
    }

    pub fn reorg(&mut self, orders: Vec<FixedBytes<32>>) -> impl Iterator<Item = AllOrders> + '_ {
        orders
            .into_iter()
            .filter_map(|id| self.id_to_orders.remove(&id))
    }

    pub fn finalized(&mut self, block: u64) -> Vec<AllOrders> {
        self.block_to_ids
            .remove(&block)
            .map(|ids| {
                ids.into_iter()
                    .filter_map(|hash| self.id_to_orders.remove(&hash))
                    .collect()
            })
            .unwrap_or_default()
    }
}
