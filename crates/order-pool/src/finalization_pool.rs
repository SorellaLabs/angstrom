use std::{collections::HashMap, iter::Iterator};

use angstrom_types::orders::PoolOrder;
use reth_primitives::B256;

use crate::common::Order;

pub struct FinalizationPool<L: PoolOrder, CL: PoolOrder, S: PoolOrder, CS: PoolOrder> {
    hashes_to_orders: HashMap<B256, Order<L, CL, S, CS>>,
    block_to_hashes:  HashMap<u64, Vec<B256>>
}

impl<L: PoolOrder, CL: PoolOrder, S: PoolOrder, CS: PoolOrder> FinalizationPool<L, CL, S, CS> {
    pub fn new() -> Self {
        Self { block_to_hashes: HashMap::default(), hashes_to_orders: HashMap::default() }
    }

    pub fn new_orders(&mut self, block: u64, orders: Vec<Order<L, CL, S, CS>>) {
        let hashes = orders
            .into_iter()
            .map(|order| {
                let hash = order.hash();
                self.hashes_to_orders.insert(hash, order);

                hash
            })
            .collect::<Vec<_>>();

        assert!(self.block_to_hashes.insert(block, hashes).is_none());
    }

    pub fn reorg(&mut self, orders: Vec<B256>) -> impl Iterator<Item = Order<L, CL, S, CS>> + '_ {
        orders
            .into_iter()
            .filter_map(|hash| self.hashes_to_orders.remove(&hash))
    }

    pub fn finalized(&mut self, block: u64) -> Vec<Order<L, CL, S, CS>> {
        self.block_to_hashes
            .remove(&block)
            .map(|hashes| {
                hashes
                    .into_iter()
                    .filter_map(|hash| self.hashes_to_orders.remove(&hash))
                    .collect()
            })
            .unwrap_or(vec![])
    }
}
