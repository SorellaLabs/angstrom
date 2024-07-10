use std::collections::HashMap;

use angstrom_types::{
    primitive::PoolId,
    sol_bindings::grouped_orders::{GroupedVanillaOrder, OrderWithStorageData}
};

use super::{parked::ParkedPool, pending::PendingPool};
use crate::limit::LimitPoolError;

pub struct LimitPool {
    pending_orders: HashMap<PoolId, PendingPool<GroupedVanillaOrder>>,
    parked_orders:  HashMap<PoolId, ParkedPool>
}

impl LimitPool {
    pub fn new(ids: &[PoolId]) -> Self {
        let parked = ids.iter().map(|id| (*id, ParkedPool::new())).collect();
        let pending = ids.iter().map(|id| (*id, PendingPool::new())).collect();

        Self { parked_orders: parked, pending_orders: pending }
    }

    pub fn add_order(
        &mut self,
        order: OrderWithStorageData<GroupedVanillaOrder>
    ) -> Result<(), LimitPoolError> {
        let pool_id = order.pool_id;
        let err = || LimitPoolError::NoPool(pool_id);

        if order.is_currently_valid {
            self.pending_orders
                .get_mut(&pool_id)
                .ok_or_else(err)?
                .add_order(order)
        } else {
            self.parked_orders
                .get_mut(&pool_id)
                .ok_or_else(err)?
                .new_order(order)
        }

        Ok(())
    }

    pub fn remove_order(
        &mut self,
        pool_id: PoolId,
        order_id: alloy_primitives::FixedBytes<32>
    ) -> Option<OrderWithStorageData<GroupedVanillaOrder>> {
        self.pending_orders
            .get_mut(&pool_id)
            .and_then(|pool| pool.remove_order(order_id))
            .or_else(|| {
                self.parked_orders
                    .get_mut(&pool_id)
                    .and_then(|pool| pool.remove_order(order_id))
            })
    }
}
