use std::{
    collections::HashMap,
    default::Default,
    fmt::Debug,
    sync::{Arc, Mutex},
    time::Instant
};

use alloy::primitives::{B256, BlockNumber, FixedBytes};
use angstrom_metrics::OrderStorageMetricsWrapper;
use angstrom_types::{
    orders::{OrderId, OrderLocation, OrderSet, OrderStatus},
    primitive::{NewInitializedPool, PoolId},
    sol_bindings::{
        grouped_orders::{AllOrders, OrderWithStorageData},
        rpc_orders::TopOfBlockOrder
    }
};

use crate::{
    PoolConfig,
    finalization_pool::FinalizationPool,
    limit::{LimitOrderPool, LimitPoolError},
    searcher::{SearcherPool, SearcherPoolError}
};

/// The Storage of all verified orders.
#[derive(Clone)]
pub struct OrderStorage {
    pub limit_orders:                Arc<Mutex<LimitOrderPool>>,
    pub searcher_orders:             Arc<Mutex<SearcherPool>>,
    pub pending_finalization_orders: Arc<Mutex<FinalizationPool>>,
    /// we store filled order hashes until they are expired time wise to ensure
    /// we don't waste processing power in the validator.
    pub filled_orders:               Arc<Mutex<HashMap<B256, Instant>>>,
    pub metrics:                     OrderStorageMetricsWrapper
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
        Self {
            filled_orders: Arc::new(Mutex::new(HashMap::default())),
            limit_orders,
            searcher_orders,
            pending_finalization_orders,
            metrics: OrderStorageMetricsWrapper::default()
        }
    }

    pub fn remove_pool(&self, key: PoolId) {
        self.searcher_orders.lock().unwrap().remove_pool(&key);
        self.limit_orders.lock().unwrap().remove_pool(&key);
    }

    pub fn fetch_status_of_order(&self, order: B256) -> Option<OrderStatus> {
        if self
            .filled_orders
            .lock()
            .expect("poisoned")
            .contains_key(&order)
            && self
                .pending_finalization_orders
                .lock()
                .expect("poisoned")
                .has_order(&order)
        {
            return Some(OrderStatus::Filled);
        }

        if self
            .searcher_orders
            .lock()
            .expect("poisoned")
            .has_order(order)
        {
            return Some(OrderStatus::Pending);
        }

        self.limit_orders
            .lock()
            .expect("poisoned")
            .get_order_status(order)
    }

    // unfortunately, any other solution is just as ugly
    // this needs to be revisited once composable orders are in place
    pub fn log_cancel_order(&self, order: &AllOrders) {
        let order_id = OrderId::from_all_orders(order, PoolId::default());
        match order_id.location {
            OrderLocation::Limit => self.metrics.incr_cancelled_vanilla_orders(),
            OrderLocation::Searcher => self.metrics.incr_cancelled_searcher_orders()
        }
    }

    pub fn get_orders_by_pool(&self, pool_id: PoolId, location: OrderLocation) -> Vec<AllOrders> {
        match location {
            OrderLocation::Limit => self
                .limit_orders
                .lock()
                .expect("lock poisoned")
                .get_all_orders_from_pool(pool_id),
            OrderLocation::Searcher => self
                .searcher_orders
                .lock()
                .expect("lock poisoned")
                .get_all_orders_from_pool(pool_id)
        }
    }

    pub fn get_order_from_id(&self, order_id: &OrderId) -> Option<OrderWithStorageData<AllOrders>> {
        match order_id.location {
            OrderLocation::Limit => self
                .limit_orders
                .lock()
                .expect("lock poisoned")
                .get_order(order_id)
                .and_then(|order| order.try_map_inner(Ok).ok()),
            OrderLocation::Searcher => self
                .searcher_orders
                .lock()
                .expect("lock poisoned")
                .get_order(order_id.pool_id, order_id.hash)
                .and_then(|order| order.try_map_inner(|inner| Ok(AllOrders::TOB(inner))).ok())
        }
    }

    pub fn cancel_order(&self, order_id: &OrderId) -> Option<OrderWithStorageData<AllOrders>> {
        if self
            .pending_finalization_orders
            .lock()
            .expect("poisoned")
            .has_order(&order_id.hash)
        {
            return None;
        }

        match order_id.location {
            angstrom_types::orders::OrderLocation::Limit => self
                .limit_orders
                .lock()
                .expect("lock poisoned")
                .remove_order(order_id)
                .inspect(|order| {
                    if order.is_vanilla() {
                        self.metrics.incr_cancelled_vanilla_orders()
                    } else {
                        self.metrics.incr_cancelled_composable_orders()
                    }
                }),
            angstrom_types::orders::OrderLocation::Searcher => self
                .searcher_orders
                .lock()
                .expect("lock poisoned")
                .remove_order(order_id)
                .map(|order| {
                    self.metrics.incr_cancelled_searcher_orders();
                    order
                        .try_map_inner(|inner| Ok(AllOrders::TOB(inner)))
                        .unwrap()
                })
        }
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

    pub fn top_tob_orders(&self) -> Vec<OrderWithStorageData<TopOfBlockOrder>> {
        let mut top_orders = Vec::new();
        let searcher_orders = self.searcher_orders.lock().expect("lock poisoned");

        for pool_id in searcher_orders.get_all_pool_ids() {
            if let Some(top_order) = searcher_orders
                .get_orders_for_pool(&pool_id)
                .unwrap_or_else(|| panic!("pool {} does not exist", pool_id))
                .iter()
                .max_by_key(|order| order.tob_reward)
                .cloned()
            {
                top_orders.push(top_order);
            }
        }

        top_orders
    }

    pub fn add_new_limit_order(
        &self,
        order: OrderWithStorageData<AllOrders>
    ) -> Result<(), LimitPoolError> {
        if order.is_vanilla() {
            self.limit_orders
                .lock()
                .expect("lock poisoned")
                .add_vanilla_order(order)?;
            self.metrics.incr_vanilla_limit_orders(1);
        } else {
            self.limit_orders
                .lock()
                .expect("lock poisoned")
                .add_composable_order(order)?;
            self.metrics.incr_composable_limit_orders(1);
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

        self.metrics.incr_searcher_orders(1);

        Ok(())
    }

    pub fn add_filled_orders(
        &self,
        block_number: BlockNumber,
        orders: Vec<OrderWithStorageData<AllOrders>>
    ) {
        let num_orders = orders.len();
        self.pending_finalization_orders
            .lock()
            .expect("poisoned")
            .new_orders(block_number, orders);

        self.metrics.incr_pending_finalization_orders(num_orders);
    }

    pub fn finalized_block(&self, block_number: BlockNumber) {
        let orders = self
            .pending_finalization_orders
            .lock()
            .expect("poisoned")
            .finalized(block_number);

        self.metrics.decr_pending_finalization_orders(orders.len());
    }

    pub fn reorg(&self, order_hashes: Vec<FixedBytes<32>>) -> Vec<OrderWithStorageData<AllOrders>> {
        let orders = self
            .pending_finalization_orders
            .lock()
            .expect("poisoned")
            .reorg(order_hashes)
            .collect::<Vec<_>>();

        self.metrics.decr_pending_finalization_orders(orders.len());
        orders
    }

    pub fn remove_order(&self, id: &OrderId) -> Option<OrderWithStorageData<AllOrders>> {
        match id.location {
            OrderLocation::Limit => self.remove_limit_order(id),
            OrderLocation::Searcher => self.remove_searcher_order(id)
        }
    }

    pub fn remove_parked_order(&self, id: &OrderId) -> Option<OrderWithStorageData<AllOrders>> {
        self.limit_orders
            .lock()
            .expect("poisoned")
            .remove_parked_order(id)
            .inspect(|order| {
                if order.is_vanilla() {
                    self.metrics.decr_vanilla_limit_orders(1);
                } else {
                    self.metrics.decr_composable_limit_orders(1);
                }
            })
    }

    pub fn remove_searcher_order(&self, id: &OrderId) -> Option<OrderWithStorageData<AllOrders>> {
        let order = self
            .searcher_orders
            .lock()
            .expect("posioned")
            .remove_order(id)
            .map(|value| {
                value
                    .try_map_inner(|v| {
                        self.metrics.decr_searcher_orders(1);
                        Ok(AllOrders::TOB(v))
                    })
                    .unwrap()
            });

        order
    }

    pub fn remove_limit_order(&self, id: &OrderId) -> Option<OrderWithStorageData<AllOrders>> {
        self.limit_orders
            .lock()
            .expect("poisoned")
            .remove_order(id)
            .inspect(|order| {
                if order.is_vanilla() {
                    self.metrics.decr_vanilla_limit_orders(1);
                } else {
                    self.metrics.decr_composable_limit_orders(1);
                }
            })
    }

    pub fn get_all_orders(&self) -> OrderSet<AllOrders, TopOfBlockOrder> {
        let limit = self.limit_orders.lock().expect("poisoned").get_all_orders();
        let searcher = self.top_tob_orders();

        OrderSet { limit, searcher }
    }

    pub fn get_all_orders_with_parked(&self) -> OrderSet<AllOrders, TopOfBlockOrder> {
        let limit = self
            .limit_orders
            .lock()
            .expect("poisoned")
            .get_all_orders_with_parked();
        let searcher = self.top_tob_orders();

        OrderSet { limit, searcher }
    }

    pub fn new_pool(&self, pool: NewInitializedPool) {
        self.limit_orders.lock().expect("poisoned").new_pool(pool);
        self.searcher_orders
            .lock()
            .expect("poisoned")
            .new_pool(pool);
    }

    pub fn remove_invalid_order(&self, order_hash: FixedBytes<32>) {
        self.searcher_orders
            .lock()
            .expect("poisoned")
            .remove_invalid_order(order_hash);
        self.limit_orders
            .lock()
            .expect("poisoned")
            .remove_invalid_order(order_hash);
    }
}
