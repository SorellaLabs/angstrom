use std::collections::HashMap;

use angstrom_metrics::SearcherOrderPoolMetricsWrapper;
use angstrom_types::{
    orders::OrderId,
    primitive::{NewInitializedPool, PoolId},
    sol_bindings::{grouped_orders::OrderWithStorageData, rpc_orders::TopOfBlockOrder}
};
use angstrom_utils::map::OwnedMap;
use pending::PendingPool;

use crate::common::SizeTracker;

mod pending;

#[allow(dead_code)]
pub const SEARCHER_POOL_MAX_SIZE: usize = 15;

#[derive(Default)]
pub struct SearcherPool {
    /// Holds all non composable searcher order pools
    searcher_orders: HashMap<PoolId, PendingPool>,
    /// The size of the current transactions.
    size:            SizeTracker,
    metrics:         SearcherOrderPoolMetricsWrapper
}

impl SearcherPool {
    pub fn new(ids: &[PoolId], max_size: Option<usize>) -> Self {
        let searcher_orders = ids.iter().map(|id| (*id, PendingPool::new())).collect();
        Self {
            searcher_orders,
            size: SizeTracker { max: max_size, current: 0 },
            metrics: SearcherOrderPoolMetricsWrapper::default()
        }
    }

    pub fn get_order(
        &self,
        pool_id: PoolId,
        order_id: alloy::primitives::FixedBytes<32>
    ) -> Option<OrderWithStorageData<TopOfBlockOrder>> {
        self.searcher_orders
            .get(&pool_id)
            .and_then(|pool| pool.get_order(order_id))
    }

    pub fn add_searcher_order(
        &mut self,
        order: OrderWithStorageData<TopOfBlockOrder>
    ) -> Result<(), SearcherPoolError> {
        let size = order.size();
        if !self.size.has_space(size) {
            return Err(SearcherPoolError::MaxSize)
        }

        let pool_id = order.pool_id;
        self.searcher_orders
            .get_mut(&pool_id)
            .ok_or_else(|| SearcherPoolError::NoPool(pool_id))?
            .add_order(order);

        self.metrics.incr_all_orders(pool_id, 1);

        Ok(())
    }

    pub fn remove_order(&mut self, id: &OrderId) -> Option<OrderWithStorageData<TopOfBlockOrder>> {
        self.searcher_orders
            .get_mut(&id.pool_id)
            .and_then(|pool| pool.remove_order(id.hash))
            .owned_map(|| self.metrics.decr_all_orders(id.pool_id, 1))
    }

    pub fn get_all_pool_ids(&self) -> Vec<PoolId> {
        self.searcher_orders.keys().cloned().collect()
    }

    pub fn get_orders_for_pool(
        &self,
        pool_id: &PoolId
    ) -> Option<Vec<OrderWithStorageData<TopOfBlockOrder>>> {
        self.searcher_orders
            .get(pool_id)
            .map(|pool| pool.get_all_orders())
    }

    pub fn get_all_orders(&self) -> Vec<OrderWithStorageData<TopOfBlockOrder>> {
        self.searcher_orders
            .values()
            .flat_map(|p| p.get_all_orders())
            .collect()
    }

    pub fn new_pool(&mut self, pool: NewInitializedPool) {
        let old_is_none = self
            .searcher_orders
            .insert(pool.id, PendingPool::new())
            .is_none();
        assert!(old_is_none);
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SearcherPoolError {
    #[error("Pool has reached max size, and order doesn't satisify replacment requirements")]
    MaxSize,
    #[error("No pool was found for address: {0} ")]
    NoPool(PoolId),
    #[error(transparent)]
    Unknown(#[from] eyre::Error)
}
