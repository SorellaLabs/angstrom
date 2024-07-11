use std::fmt::Debug;

use angstrom_types::{
    orders::OrderId,
    primitive::PoolId,
    sol_bindings::grouped_orders::{
        AllOrders, GroupedComposableOrder, GroupedVanillaOrder, OrderWithStorageData
    }
};

use self::{composable::ComposableLimitPool, standard::LimitPool};
use crate::common::SizeTracker;
mod composable;
mod parked;
mod pending;
mod standard;

pub struct LimitOrderPool {
    /// Sub-pool of all limit orders
    limit_orders:      LimitPool,
    /// Sub-pool of all composable orders
    composable_orders: ComposableLimitPool,
    /// The size of the current transactions.
    size:              SizeTracker
}

impl LimitOrderPool {
    pub fn new(ids: &[PoolId], max_size: Option<usize>) -> Self {
        Self {
            composable_orders: ComposableLimitPool::new(ids),
            limit_orders:      LimitPool::new(ids),
            size:              SizeTracker { max: max_size, current: 0 }
        }
    }

    pub fn add_composable_order(
        &mut self,
        order: OrderWithStorageData<GroupedComposableOrder>
    ) -> Result<(), LimitPoolError> {
        let size = order.size();
        if !self.size.has_space(size) {
            return Err(LimitPoolError::MaxSize)
        }

        self.composable_orders.add_order(order)
    }

    pub fn add_vanilla_order(
        &mut self,
        order: OrderWithStorageData<GroupedVanillaOrder>
    ) -> Result<(), LimitPoolError> {
        let size = order.size();
        if !self.size.has_space(size) {
            return Err(LimitPoolError::MaxSize)
        }

        self.limit_orders.add_order(order)
    }

    pub fn remove_order(&mut self, id: &OrderId) -> Option<OrderWithStorageData<AllOrders>> {
        self.limit_orders
            .remove_order(id.pool_id, id.hash)
            .and_then(|value| value.try_map_inner(|this| Ok(this.into())).ok())
            .or_else(|| {
                self.composable_orders
                    .remove_order(id.pool_id, id.hash)
                    .and_then(|value| value.try_map_inner(|this| Ok(this.into())).ok())
            })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LimitPoolError {
    #[error("Pool has reached max size, and order doesn't satisify replacment requirements")]
    MaxSize,
    #[error("No pool was found for address: {0} ")]
    NoPool(PoolId),
    #[error(transparent)]
    Unknown(#[from] eyre::Error)
}
