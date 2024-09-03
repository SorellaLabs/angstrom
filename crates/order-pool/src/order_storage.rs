use std::{
    fmt::Debug,
    sync::{Arc, Mutex}
};

use alloy_primitives::FixedBytes;
use angstrom_types::{
    orders::{OrderId, OrderSet},
    sol_bindings::{
        grouped_orders::{AllOrders, GroupedUserOrder, GroupedVanillaOrder, OrderWithStorageData},
        sol::TopOfBlockOrder
    }
};

use crate::{
    finalization_pool::FinalizationPool,
    limit::{LimitOrderPool, LimitPoolError},
    searcher::{SearcherPool, SearcherPoolError},
    PoolConfig
};

/// The Storage of all verified orders.
#[derive(Default, Clone)]
pub struct OrderStorage {
    pub limit_orders:                Arc<Mutex<LimitOrderPool>>,
    pub searcher_orders:             Arc<Mutex<SearcherPool>>,
    pub pending_finalization_orders: Arc<Mutex<FinalizationPool>>
}

impl Debug for OrderStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Simplified implementation for the moment
        write!(f, "OrderStorage")
    }
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

    /// moves all orders to the parked location if there not already.
    pub fn park_orders(&self, order_info: Vec<&OrderId>) {
        // take lock here so we don't drop between iterations.
        let mut limit_lock = self.limit_orders.lock().unwrap();
        order_info
            .into_iter()
            .for_each(|order| match order.location {
                angstrom_types::orders::OrderLocation::Limit => {
                    limit_lock.park_order(order);
                }
                angstrom_types::orders::OrderLocation::Searcher => {
                    tracing::debug!("tried to park searcher order. this is not supported");
                }
            });
    }

    pub fn add_new_limit_order(
        &self,
        order: OrderWithStorageData<GroupedUserOrder>
    ) -> Result<(), LimitPoolError> {
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
        order: OrderWithStorageData<TopOfBlockOrder>
    ) -> Result<(), SearcherPoolError> {
        self.searcher_orders
            .lock()
            .expect("lock poisoned")
            .add_searcher_order(order)?;

        Ok(())
    }

    pub fn add_filled_orders(
        &self,
        block_number: u64,
        orders: Vec<OrderWithStorageData<AllOrders>>
    ) {
        self.pending_finalization_orders
            .lock()
            .expect("poisoned")
            .new_orders(block_number, orders);
    }

    pub fn finalized_block(&self, block_number: u64) {
        self.pending_finalization_orders
            .lock()
            .expect("poisoned")
            .finalized(block_number);
    }

    pub fn reorg(&self, order_hashes: Vec<FixedBytes<32>>) -> Vec<AllOrders> {
        self.pending_finalization_orders
            .lock()
            .expect("poisoned")
            .reorg(order_hashes)
            .collect::<Vec<_>>()
    }

    pub fn remove_searcher_order(&self, id: &OrderId) -> Option<OrderWithStorageData<AllOrders>> {
        self.searcher_orders
            .lock()
            .expect("posioned")
            .remove_order(id)
            .map(|value| value.try_map_inner(|v| Ok(AllOrders::TOB(v))).unwrap())
    }

    pub fn remove_limit_order(&self, id: &OrderId) -> Option<OrderWithStorageData<AllOrders>> {
        self.limit_orders.lock().expect("poisoned").remove_order(id)
    }

    pub fn get_all_orders(&self) -> OrderSet<GroupedVanillaOrder, TopOfBlockOrder> {
        let limit = self.limit_orders.lock().expect("poisoned").get_all_orders();
        let searcher = self
            .searcher_orders
            .lock()
            .expect("poisoned")
            .get_all_orders();

        OrderSet { limit, searcher }
    }
}
