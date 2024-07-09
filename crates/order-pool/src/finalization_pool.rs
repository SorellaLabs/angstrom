use std::collections::HashMap;

use sol_bindings::grouped_orders::AllOrders;

pub struct FinalizationPool {
    id_to_orders: HashMap<u64, AllOrders>,
    block_to_ids: HashMap<u64, Vec<u64>>
}

impl FinalizationPool {
    pub fn new() -> Self {
        Self { block_to_ids: HashMap::default(), id_to_orders: HashMap::default() }
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

    pub fn reorg(&mut self, orders: Vec<u64>) -> impl Iterator<Item = AllOrders> + '_ {
        orders
            .into_iter()
            .filter_map(|id| self.id_to_orders.remove(&id))
    }

    pub fn finalized(&mut self, block: u64) -> Vec<AllOrders> {
        self.block_to_ids
            .remove(&block)
            .map(|ids| {
                ids.into_iter()
                    .filter_map(|hash| self.id_to_orders.remove(&hash))
                    .collect()
            })
            .unwrap_or_default()
    }
}
