use std::fmt::Debug;

use angstrom_types::{
    orders::OrderId,
    primitive::{NewInitializedPool, PoolId},
    sol_bindings::grouped_orders::{
        GroupedComposableOrder, GroupedUserOrder, GroupedVanillaOrder, OrderWithStorageData
    }
};

use self::{composable::ComposableLimitPool, standard::LimitPool};
use crate::common::SizeTracker;
mod composable;
mod parked;
mod pending;
mod standard;

#[derive(Default)]
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

    pub fn remove_order(&mut self, id: &OrderId) -> Option<OrderWithStorageData<GroupedUserOrder>> {
        self.limit_orders
            .remove_order(id.pool_id, id.hash)
            .and_then(|value| {
                value
                    .try_map_inner(|this| Ok(GroupedUserOrder::Vanilla(this)))
                    .ok()
            })
            .or_else(|| {
                self.composable_orders
                    .remove_order(id.pool_id, id.hash)
                    .and_then(|value| {
                        value
                            .try_map_inner(|this| Ok(GroupedUserOrder::Composable(this)))
                            .ok()
                    })
            })
    }

    pub fn get_all_orders(&self) -> Vec<OrderWithStorageData<GroupedVanillaOrder>> {
        self.limit_orders.get_all_orders()
    }

    pub fn park_order(&mut self, id: &OrderId) {
        self.limit_orders.park_order(id);
    }

    pub fn new_pool(&mut self, pool: NewInitializedPool) {
        self.limit_orders.new_pool(pool);
        self.composable_orders.new_pool(pool);
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
