use std::fmt::Debug;

use alloy::primitives::{FixedBytes, B256};
use angstrom_types::{
    orders::{OrderId, OrderStatus},
    primitive::{NewInitializedPool, PoolId},
    sol_bindings::grouped_orders::{
        AllOrders, GroupedComposableOrder, GroupedUserOrder, GroupedVanillaOrder,
        OrderWithStorageData
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

    pub fn get_order(&self, id: &OrderId) -> Option<OrderWithStorageData<GroupedUserOrder>> {
        self.limit_orders
            .get_order(id.pool_id, id.hash)
            .and_then(|value| {
                value
                    .try_map_inner(|this| Ok(GroupedUserOrder::Vanilla(this)))
                    .ok()
            })
            .or_else(|| {
                self.composable_orders
                    .get_order(id.pool_id, id.hash)
                    .and_then(|value| {
                        value
                            .try_map_inner(|this| Ok(GroupedUserOrder::Composable(this)))
                            .ok()
                    })
            })
    }

    pub fn get_order_status(&self, order_hash: B256) -> Option<OrderStatus> {
        self.limit_orders.get_order_status(order_hash)
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

    pub fn get_all_orders_from_pool(&self, pool: FixedBytes<32>) -> Vec<AllOrders> {
        self.limit_orders
            .pending_orders
            .get(&pool)
            .map(|pool| {
                pool.get_all_orders()
                    .into_iter()
                    .map(|p| p.order.into())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
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
