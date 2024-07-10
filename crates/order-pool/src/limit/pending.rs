use std::{
    cmp::Reverse,
    collections::{BTreeMap, HashMap}
};

use angstrom_types::{
    orders::OrderPriorityData, sol_bindings::grouped_orders::OrderWithStorageData
};

pub struct PendingPool<Order> {
    /// all order hashes
    orders: HashMap<u64, OrderWithStorageData<Order>>,
    /// bids are sorted descending by price, TODO: This should be binned into
    /// ticks based off of the underlying pools params
    bids:   BTreeMap<Reverse<OrderPriorityData>, u64>,
    /// asks are sorted ascending by price,  TODO: This should be binned into
    /// ticks based off of the underlying pools params
    asks:   BTreeMap<OrderPriorityData, u64>
}

impl<Order> PendingPool<Order> {
    #[allow(unused)]
    pub fn new() -> Self {
        Self { orders: HashMap::new(), bids: BTreeMap::new(), asks: BTreeMap::new() }
    }

    pub fn add_order(&mut self, order: OrderWithStorageData<Order>) {
        if order.is_bid {
            self.bids.insert(Reverse(order.priority_data), order.id);
        } else {
            self.asks.insert(order.priority_data, order.id);
        }
        self.orders.insert(order.id, order);
    }

    pub fn remove_order(&mut self, id: u64) -> Option<Order> {
        let order = self.orders.remove(&id)?;

        if order.is_bid {
            self.bids.remove(&Reverse(order.priority_data))?;
        } else {
            self.asks.remove(&order.priority_data)?;
        }

        // probably fine to strip extra data here
        Some(order.order)
    }
}
