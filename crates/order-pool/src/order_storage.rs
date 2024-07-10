use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex
    }
};

use alloy_primitives::FixedBytes;
use angstrom_types::{
    orders::OrderId,
    sol_bindings::{
        grouped_orders::{AllOrders, GroupedUserOrder, OrderWithStorageData},
        sol::TopOfBlockOrder
    }
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
    pub pending_finalization_orders: Arc<Mutex<FinalizationPool>>
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

        Self { limit_orders, searcher_orders, pending_finalization_orders }
    }

    pub fn add_new_limit_order(
        &self,
        mut order: OrderWithStorageData<GroupedUserOrder>
    ) -> Result<(), LimitPoolError> {
        let hash = order.order_hash();

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

        Ok(())
    }

    pub fn add_new_searcher_order(
        &self,
        mut order: OrderWithStorageData<TopOfBlockOrder>
    ) -> Result<(), SearcherPoolError> {
        self.searcher_orders
            .lock()
            .expect("lock poisoned")
            .add_searcher_order(order)?;

        Ok(())
    }

    pub fn add_filled_orders(&self, orders: Vec<AllOrders>) {}

    pub fn finalized_block(&self, block_number: u64) {}

    pub fn reorg(&self, order_hashes: Vec<FixedBytes<32>>) -> Vec<AllOrders> {
        vec![]
    }

    pub fn remove_searcher_order(&self, id: &OrderId) -> Option<TopOfBlockOrder> {
        None
    }

    pub fn remove_limit_order(&self, id: &OrderId) -> Option<GroupedUserOrder> {
        None
    }
}
