use std::{
    cmp::Reverse,
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Debug
};

use alloy::primitives::{B256, FixedBytes};
use angstrom_types::{
    orders::OrderPriorityData, sol_bindings::grouped_orders::OrderWithStorageData
};
use serde::{Deserialize, Serialize};
use serde_with::{DisplayFromStr, serde_as};

#[serde_as]
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PendingPool<Order: Clone + Serialize + for<'a> Deserialize<'a>> {
    /// all order hashes
    #[serde_as(as = "HashMap<DisplayFromStr, _>")]
    pub(crate) orders: HashMap<FixedBytes<32>, OrderWithStorageData<Order>>,
    /// bids are sorted descending by price, TODO: This should be binned into
    /// ticks based off of the underlying pools params
    #[serde_as(as = "Vec<(_, _)>")]
    bids:              BTreeMap<Reverse<OrderPriorityData>, FixedBytes<32>>,
    /// asks are sorted ascending by price,  TODO: This should be binned into
    /// ticks based off of the underlying pools params
    #[serde_as(as = "Vec<(_, _)>")]
    asks:              BTreeMap<OrderPriorityData, FixedBytes<32>>
}

impl<Order: Clone + Serialize + for<'a> Deserialize<'a> + Debug> PendingPool<Order> {
    pub fn new() -> Self {
        Self { orders: HashMap::new(), bids: BTreeMap::new(), asks: BTreeMap::new() }
    }

    pub fn get_order(&self, id: FixedBytes<32>) -> Option<OrderWithStorageData<Order>> {
        self.orders.get(&id).cloned()
    }

    pub fn add_order(&mut self, order: OrderWithStorageData<Order>) {
        if order.is_bid {
            self.bids
                .insert(Reverse(order.priority_data), order.order_id.hash);
        } else {
            self.asks.insert(order.priority_data, order.order_id.hash);
        }
        self.orders.insert(order.order_id.hash, order);
    }

    pub fn cancel_order(&mut self, id: FixedBytes<32>) -> bool {
        if let Some(order) = self.orders.get_mut(&id) {
            tracing::trace!(?order, "canceled user order");
            order.cancel_requested = true;

            return true;
        }

        false
    }

    pub fn remove_all_cancelled_orders(&mut self) -> Vec<OrderWithStorageData<Order>> {
        let mut res = vec![];
        let ids = self
            .orders
            .iter()
            .filter(|(_, orders)| orders.cancel_requested)
            .map(|(key, _)| *key)
            .collect::<Vec<_>>();

        for id in ids {
            if let Some(order) = self.remove_order(&id) {
                res.push(order);
            }
        }

        res
    }

    pub fn remove_order(&mut self, id: &FixedBytes<32>) -> Option<OrderWithStorageData<Order>> {
        let order = self.orders.remove(id)?;

        if order.is_bid {
            self.bids.remove(&Reverse(order.priority_data))?;
        } else {
            self.asks.remove(&order.priority_data)?;
        }

        Some(order)
    }

    pub fn get_all_orders(&self) -> Vec<OrderWithStorageData<Order>> {
        self.orders
            .values()
            .filter(|order| !order.cancel_requested)
            .cloned()
            .collect()
    }

    pub fn get_all_orders_with_cancelled(&self) -> Vec<OrderWithStorageData<Order>> {
        self.orders.values().cloned().collect()
    }

    pub fn get_all_orders_with_hashes(
        &self,
        hashes: &HashSet<B256>
    ) -> Vec<OrderWithStorageData<Order>> {
        self.orders
            .values()
            .filter_map(|order| hashes.contains(&order.order_id.hash).then_some(order))
            .cloned()
            .collect()
    }
}
