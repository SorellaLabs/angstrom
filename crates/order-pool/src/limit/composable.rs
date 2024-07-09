use std::collections::HashMap;

use angstrom_types::{
    orders::{OrderId, OrderPriorityData, PooledComposableOrder, PooledLimitOrder},
    primitive::PoolId
};
use sol_bindings::{
    grouped_orders::{GroupedVanillaOrders, OrderWithId},
    sol::SolTopOfBlockOrder,
    user_types::TopOfBlockOrder
};

use super::{pending::PendingPool, LimitPoolError};
use crate::common::ValidOrder;

pub struct ComposableLimitPool(HashMap<PoolId, PendingPool>);

impl ComposableLimitPool {
    pub fn new(ids: &[PoolId]) -> Self {
        let inner = ids.iter().map(|id| (*id, PendingPool::new())).collect();
        Self(inner)
    }

    pub fn add_order(&mut self, order: OrderWithId<GroupedVanillaOrders>) -> eyre::Result<()> {
        // let id: OrderId = order.clone().into();
        // self.0
        //     .get_mut(&id.pool_id)
        //     .ok_or_else(|| LimitPoolError::NoPool(id.pool_id, order.order.clone()))?
        //     .add_order(order);

        Ok(())
    }

    pub fn remove_order(&mut self, tx_id: &OrderId) -> Option<TopOfBlockOrder> {
        // self.0.get_mut(&tx_id.pool_id)?.remove_order(tx_id.hash)
        None
    }
}
