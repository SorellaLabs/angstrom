use guard_types::orders::{OrderId, OrderPriorityData, PoolOrder, PooledLimitOrder};

use super::{parked::ParkedPool, pending::PendingPool, LimitPoolError, OrderLocation};
use crate::{common::ValidOrder, BidsAndAsks};

pub struct LimitPool<O: PooledLimitOrder> {
    pending_orders: Vec<PendingPool<O>>,
    parked_orders:  Vec<ParkedPool<O>>
}

impl<O: PooledLimitOrder> LimitPool<O>
where
    O: PoolOrder<ValidationData = OrderPriorityData>
{
    pub fn new() -> Self {
        todo!()
    }

    pub fn add_order(&mut self, order: ValidOrder<O>) -> Result<(), LimitPoolError<O>> {
        let pool_id = order.pool_id();
        let err = || LimitPoolError::NoPool(pool_id, order.order.clone());

        if order.location.is_limit_pending() {
            self.pending_orders
                .get_mut(pool_id)
                .ok_or_else(err)?
                .add_order(order)
        } else {
            self.parked_orders
                .get_mut(pool_id)
                .ok_or_else(err)?
                .new_order(order)
        }

        Ok(())
    }

    pub fn remove_order(&mut self, order_id: &OrderId) -> Option<ValidOrder<O>> {
        match order_id.location {
            OrderLocation::LimitPending => self
                .pending_orders
                .get_mut(order_id.pool_id)
                .and_then(|pool: &mut _| pool.remove_order(order_id.hash)),
            OrderLocation::LimitParked => self
                .parked_orders
                .get_mut(order_id.pool_id)
                .and_then(|pool: &mut _| pool.remove_order(&order_id.hash)),
            _ => unreachable!()
        }
    }

    pub fn fetch_bids_asks_per_pool(&self) -> Vec<BidsAndAsks<O>> {
        self.pending_orders
            .iter()
            .map(|pool| BidsAndAsks { bids: pool.fetch_all_bids(), asks: pool.fetch_all_asks() })
            .collect()
    }
}
