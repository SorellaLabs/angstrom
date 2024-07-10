use std::collections::HashMap;

use angstrom_types::sol_bindings::grouped_orders::{GroupedVanillaOrder, OrderWithStorageData};

pub struct ParkedPool(HashMap<u64, OrderWithStorageData<GroupedVanillaOrder>>);

impl ParkedPool {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn remove_order(
        &mut self,
        order_id: u64
    ) -> Option<OrderWithStorageData<GroupedVanillaOrder>> {
        self.0.remove(&order_id)
    }

    pub fn new_order(&mut self, order: OrderWithStorageData<GroupedVanillaOrder>) {
        self.0.insert(order.id, order);
    }
}
