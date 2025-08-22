use std::{collections::HashSet, fmt::Debug};

use alloy::primitives::{B256, FixedBytes};
use angstrom_types::{
    orders::{OrderId, OrderStatus, UpdatedGas},
    primitive::{NewInitializedPool, PoolId},
    sol_bindings::grouped_orders::{AllOrders, OrderWithStorageData}
};

use self::{composable::ComposableLimitPool, standard::LimitPool};
mod composable;
mod parked;
mod pending;
mod standard;

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct LimitOrderPool {
    /// Sub-pool of all limit orders
    limit_orders:      LimitPool,
    /// Sub-pool of all composable orders
    composable_orders: ComposableLimitPool
}

impl LimitOrderPool {
    pub fn new(ids: &[PoolId]) -> Self {
        Self {
            composable_orders: ComposableLimitPool::new(ids),
            limit_orders:      LimitPool::new(ids)
        }
    }

    pub fn update_gas(&mut self, gas: &UpdatedGas) -> Vec<AllOrders> {
        self.limit_orders.update_gas(&gas.pool_id, gas)
    }

    pub fn get_order(&self, id: &OrderId) -> Option<OrderWithStorageData<AllOrders>> {
        self.limit_orders
            .get_order(id.pool_id, id.hash)
            .or_else(|| self.composable_orders.get_order(id.pool_id, id.hash))
    }

    pub fn remove_pool(&mut self, key: &PoolId) -> Vec<OrderWithStorageData<AllOrders>> {
        let mut expired_orders: Vec<_> = self
            .composable_orders
            .map
            .remove(key)
            .map(|pool| pool.get_all_orders().into_iter().collect())
            .unwrap_or_default();
        expired_orders.extend(
            self.limit_orders
                .parked_orders
                .remove(key)
                .map(|pool| pool.get_all_orders().into_iter().collect::<Vec<_>>())
                .unwrap_or_default()
        );
        expired_orders.extend(
            self.limit_orders
                .pending_orders
                .remove(key)
                .map(|pool| pool.get_all_orders().into_iter().collect::<Vec<_>>())
                .unwrap_or_default()
        );

        expired_orders
    }

    pub fn cancel_order(&mut self, id: &OrderId) -> bool {
        self.limit_orders.cancel_order(id)
    }

    pub fn remove_all_cancelled_orders(&mut self) -> Vec<OrderWithStorageData<AllOrders>> {
        self.limit_orders.remove_all_cancelled_orders()
    }

    pub fn get_order_status(&self, order_hash: B256) -> Option<OrderStatus> {
        self.limit_orders.get_order_status(order_hash)
    }

    pub fn add_composable_order(
        &mut self,
        order: OrderWithStorageData<AllOrders>
    ) -> Result<(), LimitPoolError> {
        self.composable_orders.add_order(order)
    }

    pub fn add_vanilla_order(
        &mut self,
        order: OrderWithStorageData<AllOrders>
    ) -> Result<(), LimitPoolError> {
        self.limit_orders.add_order(order)
    }

    pub fn remove_order(&mut self, id: &OrderId) -> Option<OrderWithStorageData<AllOrders>> {
        self.limit_orders
            .remove_order(id.pool_id, id.hash)
            .or_else(|| self.composable_orders.remove_order(id.pool_id, id.hash))
    }

    pub fn remove_parked_order(&mut self, id: &OrderId) -> Option<OrderWithStorageData<AllOrders>> {
        self.limit_orders
            .remove_parked_order(id.pool_id, id.hash)
            .or_else(|| self.composable_orders.remove_order(id.pool_id, id.hash))
    }

    pub fn get_all_orders(&self) -> Vec<OrderWithStorageData<AllOrders>> {
        self.limit_orders.get_all_orders()
    }

    pub fn get_all_orders_with_hashes(
        &self,
        hashes: &HashSet<B256>
    ) -> Vec<OrderWithStorageData<AllOrders>> {
        self.limit_orders.get_all_orders_with_hashes(hashes)
    }

    pub fn get_all_orders_with_parked_and_cancelled(&self) -> Vec<OrderWithStorageData<AllOrders>> {
        self.limit_orders.get_all_orders_with_cancelled_and_parked()
    }

    pub fn get_all_orders_with_parked(&self) -> Vec<OrderWithStorageData<AllOrders>> {
        self.limit_orders.get_all_orders_with_parked()
    }

    pub fn get_all_orders_from_pool(&self, pool: FixedBytes<32>) -> Vec<AllOrders> {
        self.limit_orders
            .pending_orders
            .get(&pool)
            .map(|pool| {
                pool.get_all_orders()
                    .into_iter()
                    .map(|o| o.order)
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

    pub fn remove_invalid_order(&mut self, order_hash: B256) {
        self.composable_orders.remove_invalid_order(order_hash);
        self.limit_orders.remove_invalid_order(order_hash);
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
