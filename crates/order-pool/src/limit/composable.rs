use std::collections::HashMap;

use alloy::primitives::B256;
use angstrom_metrics::ComposableLimitOrderPoolMetricsWrapper;
use angstrom_types::{
    primitive::{NewInitializedPool, PoolId},
    sol_bindings::grouped_orders::{AllOrders, OrderWithStorageData}
};
use angstrom_utils::map::OwnedMap;
use serde_with::{DisplayFromStr, serde_as};

use super::{LimitPoolError, pending::PendingPool};

#[serde_as]
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct ComposableLimitPool {
    #[serde_as(as = "HashMap<DisplayFromStr, _>")]
    pub(super) map: HashMap<PoolId, PendingPool<AllOrders>>,
    #[serde(skip)]
    metrics:        ComposableLimitOrderPoolMetricsWrapper
}

impl ComposableLimitPool {
    pub fn new(ids: &[PoolId]) -> Self {
        let map = ids.iter().map(|id| (*id, PendingPool::new())).collect();
        Self { map, metrics: ComposableLimitOrderPoolMetricsWrapper::default() }
    }

    pub fn get_order(
        &self,
        pool_id: PoolId,
        order_id: alloy::primitives::FixedBytes<32>
    ) -> Option<OrderWithStorageData<AllOrders>> {
        self.map
            .get(&pool_id)
            .and_then(|pool| pool.get_order(order_id))
    }

    pub fn add_order(
        &mut self,
        order: OrderWithStorageData<AllOrders>
    ) -> Result<(), LimitPoolError> {
        let pool_id = order.pool_id;
        self.map
            .get_mut(&pool_id)
            .ok_or_else(|| LimitPoolError::NoPool(pool_id))?
            .add_order(order);

        self.metrics.incr_all_orders(pool_id, 1);

        Ok(())
    }

    pub fn remove_order(
        &mut self,
        pool_id: PoolId,
        tx_id: alloy::primitives::FixedBytes<32>
    ) -> Option<OrderWithStorageData<AllOrders>> {
        self.map
            .get_mut(&pool_id)?
            .remove_order(&tx_id)
            .owned_map(|| self.metrics.decr_all_orders(pool_id, 1))
    }

    pub fn new_pool(&mut self, pool: NewInitializedPool) {
        let old_is_none = self.map.insert(pool.id, PendingPool::new()).is_none();
        assert!(old_is_none);
    }

    pub fn remove_invalid_order(&mut self, order_hash: B256) {
        self.map.iter_mut().for_each(|(pool_id, pool)| {
            if pool.remove_order(&order_hash).is_some() {
                self.metrics.decr_all_orders(*pool_id, 1);
            }
        });
    }
}
