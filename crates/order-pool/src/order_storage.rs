use std::{
    collections::{HashMap, HashSet},
    default::Default,
    fmt::Debug,
    sync::{Arc, Mutex},
    time::SystemTime
};

use alloy::primitives::{B256, BlockNumber, FixedBytes};
use angstrom_metrics::OrderStorageMetricsWrapper;
use angstrom_types::{
    orders::{OrderId, OrderLocation, OrderSet, OrderStatus, UpdatedGas},
    primitive::{NewInitializedPool, PoolId},
    sol_bindings::{
        grouped_orders::{AllOrders, OrderWithStorageData},
        rpc_orders::TopOfBlockOrder
    }
};
use serde::{Deserialize, Serialize};
use serde_with::{DisplayFromStr, serde_as};

use crate::{
    PoolConfig,
    finalization_pool::FinalizationPool,
    limit::{LimitOrderPool, LimitPoolError},
    searcher::{SearcherPool, SearcherPoolError}
};

/// The Storage of all verified orders.
#[serde_as]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OrderStorage {
    pub limit_orders:                Arc<Mutex<LimitOrderPool>>,
    pub searcher_orders:             Arc<Mutex<SearcherPool>>,
    pub pending_finalization_orders: Arc<Mutex<FinalizationPool>>,
    /// we store filled order hashes until they are expired time wise to ensure
    /// we don't waste processing power in the validator.
    #[serde_as(as = "Arc<Mutex<HashMap<DisplayFromStr, _>>>")]
    pub filled_orders:               Arc<Mutex<HashMap<B256, SystemTime>>>,
    #[serde(skip)]
    pub metrics:                     OrderStorageMetricsWrapper
}

impl OrderStorage {
    pub fn new(config: &PoolConfig) -> Self {
        let limit_orders = Arc::new(Mutex::new(LimitOrderPool::new(&config.ids)));
        let searcher_orders = Arc::new(Mutex::new(SearcherPool::new(&config.ids)));

        let pending_finalization_orders = Arc::new(Mutex::new(FinalizationPool::new()));

        Self {
            filled_orders: Arc::new(Mutex::new(HashMap::default())),
            limit_orders,
            searcher_orders,
            pending_finalization_orders,
            metrics: OrderStorageMetricsWrapper::new()
        }
    }

    pub fn deep_clone(&self) -> Self {
        let limit_orders = Arc::new(Mutex::new(self.limit_orders.lock().unwrap().clone()));
        let searcher_orders = Arc::new(Mutex::new(self.searcher_orders.lock().unwrap().clone()));
        let pending_finalization_orders =
            Arc::new(Mutex::new(self.pending_finalization_orders.lock().unwrap().clone()));
        let filled_orders = Arc::new(Mutex::new(self.filled_orders.lock().unwrap().clone()));

        Self {
            limit_orders,
            pending_finalization_orders,
            searcher_orders,
            filled_orders,
            metrics: OrderStorageMetricsWrapper::new()
        }
    }

    pub fn remove_pool(&self, key: PoolId) -> Vec<OrderWithStorageData<AllOrders>> {
        let mut orders = self.searcher_orders.lock().unwrap().remove_pool(&key);
        orders.extend(self.limit_orders.lock().unwrap().remove_pool(&key));

        orders
    }

    pub fn apply_new_gas_and_return_blocked_orders(
        &self,
        gas_updates: Vec<UpdatedGas>
    ) -> Vec<AllOrders> {
        let mut limit_lock = self.limit_orders.lock().unwrap();

        gas_updates
            .iter()
            .flat_map(|gas_update| limit_lock.update_gas(gas_update))
            .collect()
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

    pub fn remove_order_from_id(
        &self,
        order_id: &OrderId
    ) -> Option<OrderWithStorageData<AllOrders>> {
        match order_id.location {
            OrderLocation::Limit => self
                .limit_orders
                .lock()
                .expect("lock poisoned")
                .remove_order(order_id)
                .and_then(|order| order.try_map_inner(Ok).ok()),
            OrderLocation::Searcher => self
                .searcher_orders
                .lock()
                .expect("lock poisoned")
                .remove_order(order_id)
                .and_then(|order| order.try_map_inner(|inner| Ok(AllOrders::TOB(inner))).ok())
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

    pub fn purge_tob_orders(&self) -> Vec<OrderWithStorageData<TopOfBlockOrder>> {
        self.searcher_orders
            .lock()
            .expect("lock poisoned")
            .remove_all_cancelled_orders()
    }

    pub fn purge_user_orders(&self) -> Vec<OrderWithStorageData<AllOrders>> {
        self.limit_orders
            .lock()
            .expect("lock poisoned")
            .remove_all_cancelled_orders()
    }

    pub fn cancel_order(&self, order_id: &OrderId) -> bool {
        if self
            .pending_finalization_orders
            .lock()
            .expect("poisoned")
            .has_order(&order_id.hash)
        {
            return true;
        }

        match order_id.location {
            angstrom_types::orders::OrderLocation::Limit => self
                .limit_orders
                .lock()
                .expect("lock poisoned")
                .cancel_order(order_id),
            angstrom_types::orders::OrderLocation::Searcher => self
                .searcher_orders
                .lock()
                .expect("lock poisoned")
                .cancel_order(order_id)
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

    pub fn all_top_tob_orders(&self) -> Vec<OrderWithStorageData<TopOfBlockOrder>> {
        let searcher_orders = self.searcher_orders.lock().expect("lock poisoned");
        searcher_orders
            .get_all_pool_ids()
            .flat_map(|pool_id| {
                searcher_orders
                    .get_orders_for_pool_including_canceled(&pool_id)
                    .unwrap_or_else(|| panic!("pool {pool_id} does not exist"))
            })
            .collect()
    }

    pub fn top_tob_orders(&self) -> Vec<OrderWithStorageData<TopOfBlockOrder>> {
        let searcher_orders = self.searcher_orders.lock().expect("lock poisoned");

        searcher_orders
            .get_all_pool_ids()
            .flat_map(|pool_id| {
                searcher_orders
                    .get_orders_for_pool(&pool_id)
                    .unwrap_or_else(|| panic!("pool {pool_id} does not exist"))
            })
            .collect()
    }

    pub fn top_orders_with_hashes(
        &self,
        hashes: &HashSet<B256>
    ) -> Vec<OrderWithStorageData<TopOfBlockOrder>> {
        let searcher_orders = self.searcher_orders.lock().expect("lock poisoned");

        searcher_orders
            .get_all_pool_ids()
            .flat_map(|pool_id| {
                searcher_orders
                    .get_orders_for_pool_with_hashes(&pool_id, hashes)
                    .unwrap_or_else(|| panic!("pool {pool_id} does not exist"))
            })
            .collect()
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
            .or_else(|| self.remove_searcher_order(id))
    }

    pub fn remove_searcher_order(&self, id: &OrderId) -> Option<OrderWithStorageData<AllOrders>> {
        self.searcher_orders
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
            })
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

    pub fn get_all_orders_with_ingoing_cancellations(
        &self
    ) -> OrderSet<AllOrders, TopOfBlockOrder> {
        // Given that this is used for generating the proposal, we also
        // don't wanna ignore newly blocked orders.
        let limit = self
            .limit_orders
            .lock()
            .expect("poisoned")
            .get_all_orders_with_parked_and_cancelled();
        let searcher = self.all_top_tob_orders();

        OrderSet { limit, searcher }
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

    pub fn get_all_orders_with_hashes(
        &self,
        limit_hashes: &HashSet<B256>,
        searcher_hashes: &HashSet<B256>
    ) -> OrderSet<AllOrders, TopOfBlockOrder> {
        let limit = self
            .limit_orders
            .lock()
            .expect("poisoned")
            .get_all_orders_with_hashes(limit_hashes);
        let searcher = self.top_orders_with_hashes(searcher_hashes);

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
