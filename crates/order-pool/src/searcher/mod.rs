use std::collections::{HashMap, HashSet};

use alloy::primitives::{B256, FixedBytes};
use angstrom_metrics::SearcherOrderPoolMetricsWrapper;
use angstrom_types::{
    orders::OrderId,
    primitive::{NewInitializedPool, PoolId},
    sol_bindings::{grouped_orders::OrderWithStorageData, rpc_orders::TopOfBlockOrder}
};
use angstrom_utils::map::OwnedMap;
use pending::PendingPool;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

use crate::AllOrders;

mod pending;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde_as]
pub struct SearcherPool {
    /// Holds all non composable searcher order pools
    #[serde_as(as = "HashMap<DisplayFromStr, _>")]
    searcher_orders: HashMap<PoolId, PendingPool>,
    #[serde(skip)]
    metrics:         SearcherOrderPoolMetricsWrapper
}

impl SearcherPool {
    pub fn new(ids: &[PoolId]) -> Self {
        let searcher_orders = ids.iter().map(|id| (*id, PendingPool::new())).collect();
        Self { searcher_orders, metrics: SearcherOrderPoolMetricsWrapper::default() }
    }

    pub fn get_all_orders_from_pool(&self, pool: FixedBytes<32>) -> Vec<AllOrders> {
        self.searcher_orders
            .get(&pool)
            .map(|pool| {
                pool.get_all_orders()
                    .into_iter()
                    .map(|p| p.order.into())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    }

    pub fn has_order(&self, order_hash: B256) -> bool {
        self.searcher_orders
            .values()
            .find_map(|pool| pool.get_order(order_hash))
            .map(|_| true)
            .unwrap_or_default()
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
        let pool_id = order.pool_id;
        self.searcher_orders
            .get_mut(&pool_id)
            .ok_or_else(|| SearcherPoolError::NoPool(pool_id))?
            .add_order(order);

        self.metrics.incr_all_orders(pool_id, 1);

        Ok(())
    }

    pub fn cancel_order(&mut self, id: &OrderId) -> bool {
        if let Some(pool) = self.searcher_orders.get_mut(&id.pool_id) {
            return pool.cancel_order(id.hash);
        }

        false
    }

    pub fn remove_all_cancelled_orders(&mut self) -> Vec<OrderWithStorageData<TopOfBlockOrder>> {
        self.searcher_orders
            .values_mut()
            .flat_map(|pool| pool.remove_all_cancelled_orders())
            .collect()
    }

    pub fn remove_order(&mut self, id: &OrderId) -> Option<OrderWithStorageData<TopOfBlockOrder>> {
        self.searcher_orders
            .get_mut(&id.pool_id)
            .and_then(|pool| pool.remove_order(id.hash))
            .owned_map(|| {
                self.metrics.decr_all_orders(id.pool_id, 1);
            })
    }

    pub fn get_all_pool_ids(&self) -> impl Iterator<Item = PoolId> + '_ {
        self.searcher_orders.keys().copied()
    }

    pub fn get_orders_for_pool(
        &self,
        pool_id: &PoolId
    ) -> Option<Vec<OrderWithStorageData<TopOfBlockOrder>>> {
        self.searcher_orders
            .get(pool_id)
            .map(|pool| pool.get_all_orders())
    }

    pub fn get_orders_for_pool_including_canceled(
        &self,
        pool_id: &PoolId
    ) -> Option<Vec<OrderWithStorageData<TopOfBlockOrder>>> {
        self.searcher_orders
            .get(pool_id)
            .map(|pool| pool.get_all_orders_with_cancelled())
    }

    pub fn get_orders_for_pool_with_hashes(
        &self,
        pool_id: &PoolId,
        hashes: &HashSet<B256>
    ) -> Option<Vec<OrderWithStorageData<TopOfBlockOrder>>> {
        self.searcher_orders
            .get(pool_id)
            .map(|pool| pool.get_all_orders_with_hashes(hashes))
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

    pub fn remove_pool(&mut self, key: &PoolId) -> Vec<OrderWithStorageData<AllOrders>> {
        self.searcher_orders
            .remove(key)
            .map(|pool| {
                pool.get_all_orders()
                    .into_iter()
                    .map(|o| o.try_map_inner(|o| Ok(AllOrders::TOB(o))).unwrap())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    }

    pub fn remove_invalid_order(&mut self, order_hash: B256) {
        self.searcher_orders.iter_mut().for_each(|(pool_id, pool)| {
            if pool.remove_order(order_hash).is_some() {
                self.metrics.decr_all_orders(*pool_id, 1);
            }
        });
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
