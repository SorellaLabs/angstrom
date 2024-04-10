use std::collections::HashMap;

use angstrom_types::{
    orders::{OrderId, OrderPriorityData, PoolOrder, PooledLimitOrder},
    primitive::PoolId
};

use super::{parked::ParkedPool, pending::PendingPool, LimitPoolError, OrderLocation};
use crate::{common::ValidOrder, BidsAndAsks};

pub struct LimitPool<O: PooledLimitOrder> {
    pending_orders: HashMap<PoolId, PendingPool<O>>,
    parked_orders:  HashMap<PoolId, ParkedPool<O>>
}

impl<O: PooledLimitOrder> LimitPool<O>
where
    O: PoolOrder<ValidationData = OrderPriorityData>
{
    pub fn new(ids: &[PoolId]) -> Self {
        let parked = ids.iter().map(|id| (*id, ParkedPool::new())).collect();
        let pending = ids.iter().map(|id| (*id, PendingPool::new())).collect();

        Self { parked_orders: parked, pending_orders: pending }
    }

    pub fn add_order(&mut self, order: ValidOrder<O>) -> Result<(), LimitPoolError<O>> {
        let pool_id = order.pool_id();
        let err = || LimitPoolError::NoPool(pool_id, order.order.clone());

        if order.location.is_limit_pending() {
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

    pub fn remove_order(&mut self, order_id: &OrderId) -> Option<ValidOrder<O>> {
        match order_id.location {
            OrderLocation::LimitPending => self
                .pending_orders
                .get_mut(&order_id.pool_id)
                .and_then(|pool: &mut _| pool.remove_order(order_id.hash)),
            OrderLocation::LimitParked => self
                .parked_orders
                .get_mut(&order_id.pool_id)
                .and_then(|pool: &mut _| pool.remove_order(&order_id.hash)),
            _ => unreachable!()
        }
    }

    pub fn fetch_bids_asks_per_pool(&self) -> Vec<BidsAndAsks<O>> {
        self.pending_orders
            .values()
            .map(|pool| BidsAndAsks { bids: pool.fetch_all_bids(), asks: pool.fetch_all_asks() })
            .collect()
    }
}
