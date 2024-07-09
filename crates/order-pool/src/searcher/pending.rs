use std::{
    cmp::Reverse,
    collections::{BTreeMap, HashMap}
};

use alloy_primitives::B256;
use angstrom_types::orders::{
    OrderLocation, OrderPriorityData, PooledSearcherOrder, SearcherPriorityData, ValidatedOrder
};
use sol_bindings::{grouped_orders::OrderWithId, user_types::TopOfBlockOrder};

use super::{SearcherPoolError, SEARCHER_POOL_MAX_SIZE};
use crate::common::{SizeTracker, ValidOrder};

pub struct PendingPool {
    /// all order hashes
    orders: HashMap<u128, TopOfBlockOrder>,
    /// bids are sorted descending by price, TODO: This should be binned into
    /// ticks based off of the underlying pools params
    bids:   BTreeMap<Reverse<OrderPriorityData>, u128>,
    /// asks are sorted ascending by price,  TODO: This should be binned into
    /// ticks based off of the underlying pools params
    asks:   BTreeMap<OrderPriorityData, u128>
}

impl PendingPool {
    #[allow(unused)]
    pub fn new() -> Self {
        Self { orders: HashMap::new(), bids: BTreeMap::new(), asks: BTreeMap::new() }
    }

    pub fn add_order(&mut self, order: OrderWithId<TopOfBlockOrder>) {
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
