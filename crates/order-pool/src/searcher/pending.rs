use std::{
    cmp::Reverse,
    collections::{BTreeMap, HashMap}
};

use alloy::primitives::FixedBytes;
use angstrom_types::{
    orders::OrderPriorityData,
    sol_bindings::{grouped_orders::OrderWithStorageData, rpc_orders::TopOfBlockOrder}
};
use serde::{Deserialize, Serialize};
use serde_with::{DisplayFromStr, serde_as};

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingPool {
    /// all order hashes

    #[serde_as(as = "HashMap<DisplayFromStr, _>")]
    orders: HashMap<FixedBytes<32>, OrderWithStorageData<TopOfBlockOrder>>,
    /// bids are sorted descending by price,
    #[serde_as(as = "Vec<(_, _)>")]
    bids:   BTreeMap<Reverse<OrderPriorityData>, FixedBytes<32>>,
    /// asks are sorted ascending by price,  
    #[serde_as(as = "Vec<(_, _)>")]
    asks:   BTreeMap<OrderPriorityData, FixedBytes<32>>
}

impl PendingPool {
    #[allow(unused)]
    pub fn new() -> Self {
        Self { orders: HashMap::new(), bids: BTreeMap::new(), asks: BTreeMap::new() }
    }

    pub fn get_order(&self, id: FixedBytes<32>) -> Option<OrderWithStorageData<TopOfBlockOrder>> {
        self.orders.get(&id).cloned()
    }

    pub fn add_order(&mut self, order: OrderWithStorageData<TopOfBlockOrder>) {
        if order.is_bid {
            self.bids
                .insert(Reverse(order.priority_data), order.order_id.hash);
        } else {
            self.asks.insert(order.priority_data, order.order_id.hash);
        }
        self.orders.insert(order.order_id.hash, order);
    }

    pub fn remove_order(
        &mut self,
        id: FixedBytes<32>
    ) -> Option<OrderWithStorageData<TopOfBlockOrder>> {
        let order = self.orders.remove(&id)?;

        if order.is_bid {
            self.bids.remove(&Reverse(order.priority_data))?;
        } else {
            self.asks.remove(&order.priority_data)?;
        }

        // probably fine to strip extra data here
        Some(order)
    }

    pub fn get_all_orders(&self) -> Vec<OrderWithStorageData<TopOfBlockOrder>> {
        self.orders.values().cloned().collect()
    }
}
