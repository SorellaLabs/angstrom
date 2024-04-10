use std::collections::HashMap;

use alloy_primitives::B256;
use angstrom_types::orders::PoolOrder;

use crate::common::ValidOrder;

pub struct ParkedPool<O: PoolOrder>(HashMap<B256, ValidOrder<O>>);

impl<O: PoolOrder> ParkedPool<O> {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn remove_order(&mut self, order_hash: &B256) -> Option<ValidOrder<O>> {
        self.0.remove(order_hash)
    }

    pub fn new_order(&mut self, order: ValidOrder<O>) {
        self.0.insert(order.hash(), order);
    }
}
