use std::collections::HashMap;

use angstrom_types::{
    orders::{OrderId, OrderPriorityData, PooledComposableOrder, PooledLimitOrder},
    primitive::PoolId
};

use super::{pending::PendingPool, LimitPoolError};
use crate::{common::ValidOrder, BidsAndAsks};

pub struct ComposableLimitPool<C: PooledComposableOrder + PooledLimitOrder>(
    HashMap<PoolId, PendingPool<C>>
);

impl<O: PooledComposableOrder + PooledLimitOrder> ComposableLimitPool<O>
where
    O: PooledComposableOrder<ValidationData = OrderPriorityData>
{
    pub fn new(ids: &[PoolId]) -> Self {
        let inner = ids.iter().map(|id| (*id, PendingPool::new())).collect();
        Self(inner)
    }

    pub fn add_order(&mut self, order: ValidOrder<O>) -> Result<(), LimitPoolError<O>> {
        let id: OrderId = order.clone().into();
        self.0
            .get_mut(&id.pool_id)
            .ok_or_else(|| LimitPoolError::NoPool(id.pool_id, order.order.clone()))?
            .add_order(order);

        Ok(())
    }

    pub fn remove_order(&mut self, tx_id: &OrderId) -> Option<ValidOrder<O>> {
        self.0.get_mut(&tx_id.pool_id)?.remove_order(tx_id.hash)
    }

    pub fn fetch_bids_asks_per_pool(&self) -> Vec<BidsAndAsks<O>> {
        self.0
            .values()
            .map(|pool| BidsAndAsks { bids: pool.fetch_all_bids(), asks: pool.fetch_all_asks() })
            .collect()
    }
}
