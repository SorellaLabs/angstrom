use std::collections::{HashMap, HashSet};

use alloy::primitives::FixedBytes;
use angstrom_types::sol_bindings::grouped_orders::{AllOrders, OrderWithStorageData};
use serde::{Deserialize, Serialize};
use serde_with::{DisplayFromStr, serde_as};

#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ParkedPool(
    #[serde_as(as = "HashMap<DisplayFromStr, _>")]
    HashMap<FixedBytes<32>, OrderWithStorageData<AllOrders>>
);

impl ParkedPool {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn get_all_orders(&self) -> Vec<OrderWithStorageData<AllOrders>> {
        self.0.values().cloned().collect()
    }

    pub fn get_order(&self, order_id: FixedBytes<32>) -> Option<OrderWithStorageData<AllOrders>> {
        self.0.get(&order_id).cloned()
    }

    pub fn cancel_order(&mut self, id: FixedBytes<32>) -> bool {
        if let Some(order) = self.0.get_mut(&id) {
            tracing::trace!(?order, "canceled parked order");
            order.cancel_requested = true;

            return true;
        }

        false
    }

    pub fn remove_all_cancelled_orders(&mut self) -> Vec<OrderWithStorageData<AllOrders>> {
        let mut res = vec![];
        let ids = self
            .0
            .iter()
            .filter(|(_, orders)| orders.cancel_requested)
            .map(|(key, _)| *key)
            .collect::<Vec<_>>();
        for id in ids {
            if let Some(order) = self.remove_order(id) {
                res.push(order);
            }
        }

        res
    }

    pub fn get_all_orders_with_cancelled(&self) -> Vec<OrderWithStorageData<AllOrders>> {
        self.0.values().cloned().collect()
    }

    pub fn remove_order(
        &mut self,
        order_id: FixedBytes<32>
    ) -> Option<OrderWithStorageData<AllOrders>> {
        self.0.remove(&order_id)
    }

    pub fn new_order(&mut self, order: OrderWithStorageData<AllOrders>) {
        self.0.insert(order.order_hash(), order);
    }

    pub fn get_all_orders_with_hashes(
        &self,
        hashes: &HashSet<FixedBytes<32>>
    ) -> Vec<OrderWithStorageData<AllOrders>> {
        self.0
            .values()
            .filter_map(|order| hashes.contains(&order.order_id.hash).then_some(order))
            .cloned()
            .collect()
    }
}
