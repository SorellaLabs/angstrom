use std::collections::HashMap;

use angstrom_types::orders::PoolOrder;
use reth_primitives::B256;
use sol_bindings::grouped_orders::AllOrders;

use crate::common::Order;

pub struct FinalizationPool {
    hashes_to_orders: HashMap<u128, AllOrders>,
    block_to_hashes:  HashMap<u64, Vec<u128>>
}

impl FinalizationPool {
    pub fn new() -> Self {
        Self { block_to_hashes: HashMap::default(), hashes_to_orders: HashMap::default() }
    }

    pub fn new_orders(&mut self, block: u64, orders: Vec<AllOrders>) {
        // let hashes = orders
        //     .into_iter()
        //     .map(|order| {
        //         let hash = order.hash();
        //         self.hashes_to_orders.insert(hash, order);
        //
        //         hash
        //     })
        //     .collect::<Vec<_>>();
        //
        // assert!(self.block_to_hashes.insert(block, hashes).is_none());
    }

    pub fn reorg(&mut self, orders: Vec<u128>) -> impl Iterator<Item = AllOrders> + '_ {
        orders
            .into_iter()
            .filter_map(|hash| self.hashes_to_orders.remove(&hash))
    }

    pub fn finalized(&mut self, block: u64) -> Vec<AllOrders> {
        self.block_to_hashes
            .remove(&block)
            .map(|hashes| {
                hashes
                    .into_iter()
                    .filter_map(|hash| self.hashes_to_orders.remove(&hash))
                    .collect()
            })
            .unwrap_or_default()
    }
}
