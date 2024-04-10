use std::fmt::Debug;

use angstrom_types::{
    orders::{OrderId, OrderLocation, OrderPriorityData, PooledComposableOrder, PooledLimitOrder},
    primitive::PoolId
};

use self::{composable::ComposableLimitPool, limit::LimitPool};
use crate::{
    common::{SizeTracker, ValidOrder},
    BidsAndAsks
};

mod composable;
mod limit;
mod parked;
mod pending;

pub struct LimitOrderPool<O, C>
where
    O: PooledLimitOrder,
    C: PooledComposableOrder + PooledLimitOrder
{
    /// Sub-pool of all limit orders
    limit_orders:      LimitPool<O>,
    /// Sub-pool of all composable orders
    composable_orders: ComposableLimitPool<C>,
    /// The size of the current transactions.
    size:              SizeTracker
}

impl<O: PooledLimitOrder, C: PooledComposableOrder + PooledLimitOrder> LimitOrderPool<O, C>
where
    O: PooledLimitOrder<ValidationData = OrderPriorityData>,
    C: PooledComposableOrder + PooledLimitOrder<ValidationData = OrderPriorityData>
{
    pub fn new(ids: &[PoolId], max_size: Option<usize>) -> Self {
        Self {
            composable_orders: ComposableLimitPool::new(ids),
            limit_orders:      LimitPool::new(ids),
            size:              SizeTracker { max: max_size, current: 0 }
        }
    }

    pub fn add_composable_order(&mut self, order: ValidOrder<C>) -> Result<(), LimitPoolError<C>> {
        let size = order.size();
        if !self.size.has_space(size) {
            return Err(LimitPoolError::MaxSize(order.order))
        }

        self.composable_orders.add_order(order)?;

        Ok(())
    }

    pub fn add_limit_order(&mut self, order: ValidOrder<O>) -> Result<(), LimitPoolError<O>> {
        let size = order.size();
        if !self.size.has_space(size) {
            return Err(LimitPoolError::MaxSize(order.order))
        }
        self.limit_orders.add_order(order)?;

        Ok(())
    }

    pub fn fetch_all_vanilla_orders(&self) -> Vec<BidsAndAsks<O>> {
        self.limit_orders.fetch_bids_asks_per_pool()
    }

    pub fn fetch_all_composable_orders(&self) -> Vec<BidsAndAsks<C>> {
        self.composable_orders.fetch_bids_asks_per_pool()
    }

    pub fn remove_limit_order(&mut self, order_id: &OrderId) -> Option<ValidOrder<O>> {
        self.limit_orders.remove_order(order_id)
    }

    pub fn remove_composable_limit_order(&mut self, order_id: &OrderId) -> Option<ValidOrder<C>> {
        self.composable_orders.remove_order(order_id)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LimitPoolError<O: Debug> {
    #[error(
        "Pool has reached max size, and order doesn't satisify replacment requirements, Order: \
         {0:#?}"
    )]
    MaxSize(O),
    #[error("No pool was found for address: {0} Order: {1:#?}")]
    NoPool(PoolId, O)
}
