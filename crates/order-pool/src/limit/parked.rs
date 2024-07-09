use std::collections::HashMap;

use alloy_primitives::B256;
use angstrom_types::orders::{OrderId, PoolOrder};
use sol_bindings::grouped_orders::{GroupedVanillaOrders, OrderWithId};

use crate::common::ValidOrder;

pub struct ParkedPool(HashMap<u128, GroupedVanillaOrders>);

impl ParkedPool {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn remove_order(&mut self, order_id: &u128) -> Option<GroupedVanillaOrders> {
        self.0.remove(order_id)
    }

    pub fn new_order(&mut self, order: OrderWithId<GroupedVanillaOrders>) {
        self.0.insert(order.id, order.order);
    }
}
