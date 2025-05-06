use std::collections::HashMap;

use alloy::primitives::FixedBytes;
use angstrom_types::sol_bindings::grouped_orders::{AllOrders, OrderWithStorageData};

pub struct ParkedPool(HashMap<FixedBytes<32>, OrderWithStorageData<AllOrders>>);

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

    pub fn remove_order(
        &mut self,
        order_id: FixedBytes<32>
    ) -> Option<OrderWithStorageData<AllOrders>> {
        self.0.remove(&order_id)
    }

    pub fn new_order(&mut self, order: OrderWithStorageData<AllOrders>) {
        self.0.insert(order.order_hash(), order);
    }
}
