use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex
    }
};

use angstrom_types::sol_bindings::{
    grouped_orders::{GroupedUserOrder, OrderWithStorageData},
    sol::TopOfBlockOrder
};
use reth_primitives::B256;

use crate::{
    finalization_pool::FinalizationPool,
    limit::{LimitOrderPool, LimitPoolError},
    searcher::{SearcherPool, SearcherPoolError},
    PoolConfig
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
        let limit_orders = Arc::new(Mutex::new(LimitOrderPool::new(
            &config.ids,
            Some(config.lo_pending_limit.max_size)
        )));
        let searcher_orders = Arc::new(Mutex::new(SearcherPool::new(
            &config.ids,
            Some(config.s_pending_limit.max_size)
        )));
        let pending_finalization_orders = Arc::new(Mutex::new(FinalizationPool::new()));
        let order_hash_to_id = Arc::new(Mutex::new(HashMap::new()));
        let order_id_nonce = Arc::new(AtomicU64::new(0));

        Self {
            limit_orders,
            searcher_orders,
            pending_finalization_orders,
            order_hash_to_id,
            order_id_nonce
        }
    }

    pub fn add_new_limit_order(
        &self,
        mut order: OrderWithStorageData<GroupedUserOrder>
    ) -> Result<u64, LimitPoolError> {
        let id = self.order_id_nonce.fetch_add(1, Ordering::SeqCst);
        let hash = order.order_hash();

        // set id
        order.id = id;
        if order.is_vanilla() {
            let mapped_order = order.try_map_inner(|this| {
                let GroupedUserOrder::Vanilla(order) = this else {
                    return Err(eyre::eyre!("unreachable"))
                };
                Ok(order)
            })?;

            self.limit_orders
                .lock()
                .expect("lock poisoned")
                .add_vanilla_order(mapped_order)?;
        } else {
            let mapped_order = order.try_map_inner(|this| {
                let GroupedUserOrder::Composable(order) = this else {
                    return Err(eyre::eyre!("unreachable"))
                };
                Ok(order)
            })?;

            self.limit_orders
                .lock()
                .expect("lock poisoned")
                .add_composable_order(mapped_order)?;
        }

        self.order_hash_to_id
            .lock()
            .expect("poisoned lock")
            .insert(hash, id);

        Ok(id)
    }

    pub fn add_new_searcher_order(
        &self,
        mut order: OrderWithStorageData<TopOfBlockOrder>
    ) -> Result<u64, SearcherPoolError> {
        let id = self.order_id_nonce.fetch_add(1, Ordering::SeqCst);
        let hash = order.order.order_hash();

        // set id
        order.id = id;
        self.searcher_orders
            .lock()
            .expect("lock poisoned")
            .add_searcher_order(order)?;

        self.order_hash_to_id
            .lock()
            .expect("poisoned lock")
            .insert(hash, id);

        Ok(id)
    }
}
