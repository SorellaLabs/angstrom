use std::collections::HashMap;

use alloy::primitives::FixedBytes;
use angstrom_types::sol_bindings::grouped_orders::{GroupedVanillaOrder, OrderWithStorageData};

pub struct ParkedPool(HashMap<FixedBytes<32>, OrderWithStorageData<GroupedVanillaOrder>>);

impl ParkedPool {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn get_order(
        &self,
        order_id: FixedBytes<32>
    ) -> Option<OrderWithStorageData<GroupedVanillaOrder>> {
        self.0.get(&order_id).cloned()
    }

    pub fn remove_order(
        &mut self,
        order_id: FixedBytes<32>
    ) -> Option<OrderWithStorageData<GroupedVanillaOrder>> {
        self.0.remove(&order_id)
    }

    pub fn new_order(&mut self, order: OrderWithStorageData<GroupedVanillaOrder>) {
        self.0.insert(order.hash(), order);
    }
}
