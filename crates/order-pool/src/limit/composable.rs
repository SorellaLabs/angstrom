use std::collections::HashMap;

use angstrom_types::{
    primitive::PoolId,
    sol_bindings::grouped_orders::{GroupedComposableOrder, OrderWithStorageData}
};

use super::{pending::PendingPool, LimitPoolError};

pub struct ComposableLimitPool(HashMap<PoolId, PendingPool<GroupedComposableOrder>>);

impl ComposableLimitPool {
    pub fn new(ids: &[PoolId]) -> Self {
        let inner = ids.iter().map(|id| (*id, PendingPool::new())).collect();
        Self(inner)
    }

    pub fn add_order(
        &mut self,
        order: OrderWithStorageData<GroupedComposableOrder>
    ) -> Result<(), LimitPoolError> {
        let pool_id = order.pool_id;
        self.0
            .get_mut(&pool_id)
            .ok_or_else(|| LimitPoolError::NoPool(pool_id))?
            .add_order(order);

        Ok(())
    }

    pub fn remove_order(
        &mut self,
        pool_id: PoolId,
        tx_id: alloy_primitives::FixedBytes<32>
    ) -> Option<OrderWithStorageData<GroupedComposableOrder>> {
        self.0.get_mut(&pool_id)?.remove_order(tx_id)
    }
}
