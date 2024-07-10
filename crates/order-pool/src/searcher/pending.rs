use std::{
    cmp::Reverse,
    collections::{BTreeMap, HashMap}
};

use angstrom_types::{
    orders::OrderPriorityData,
    sol_bindings::{grouped_orders::OrderWithStorageData, user_types::TopOfBlockOrder}
};

pub struct PendingPool {
    /// all order hashes
    orders: HashMap<u64, TopOfBlockOrder>,
    /// bids are sorted descending by price, TODO: This should be binned into
    /// ticks based off of the underlying pools params
    bids:   BTreeMap<Reverse<OrderPriorityData>, u64>,
    /// asks are sorted ascending by price,  TODO: This should be binned into
    /// ticks based off of the underlying pools params
    asks:   BTreeMap<OrderPriorityData, u64>
}

impl PendingPool {
    #[allow(unused)]
    pub fn new() -> Self {
        Self { orders: HashMap::new(), bids: BTreeMap::new(), asks: BTreeMap::new() }
    }

    pub fn add_order(&mut self, order: OrderWithStorageData<TopOfBlockOrder>) {
        // let hash = order.hash();
        // let priority = order.priority_data();
        //
        // if order.is_bid() {
        //     self.bids.insert(Reverse(priority), hash);
        // } else {
        //     self.asks.insert(priority, hash);
        // }
        //
        // self.orders.insert(hash, order.clone());
    }

    pub fn remove_order(&mut self, hash: u128) -> Option<TopOfBlockOrder> {
        // let order = self.orders.remove(&hash)?;
        // let priority = order.priority_data();
        //
        // if order.is_bid() {
        //     self.bids.remove(&Reverse(priority))?;
        // } else {
        //     self.asks.remove(&priority)?;
        // }
        //
        // Some(order)
        None
    }
}
