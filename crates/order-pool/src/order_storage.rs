use std::{
    collections::HashMap,
    sync::{atomic::AtomicU64, Arc, Mutex}
};

use reth_primitives::B256;

use crate::{
    finalization_pool::FinalizationPool, limit::LimitOrderPool, searcher::SearcherPool, PoolConfig
};

/// The Storage of all verified orders.
#[derive(Clone)]
pub struct OrderStorage {
    pub limit_orders:                Arc<Mutex<LimitOrderPool>>,
    pub searcher_orders:             Arc<Mutex<SearcherPool>>,
    pub pending_finalization_orders: Arc<Mutex<FinalizationPool>>,
    pub order_hash_to_id:            Arc<Mutex<HashMap<B256, u64>>>,
    pub order_id_nonce:              Arc<AtomicU64>
}

impl OrderStorage {
    pub fn new(config: &PoolConfig) -> Self {
        todo!()
    }
}
