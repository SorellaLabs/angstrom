use std::fmt::Debug;

use angstrom_types::{
    primitive::PoolId,
    sol_bindings::{
        grouped_orders::{GroupedVanillaOrders, OrderWithId},
        user_types::TopOfBlockOrder
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
        order: OrderWithId<TopOfBlockOrder>
    ) -> eyre::Result<()> {
        // let size = order.size();
        // if !self.size.has_space(size) {
        //     return Err(LimitPoolError::MaxSize(order.order))
        // }
        //
        // self.composable_orders.add_order(order)?;

        Ok(())
    }

    pub fn add_limit_order(
        &mut self,
        order: OrderWithId<GroupedVanillaOrders>
    ) -> eyre::Result<()> {
        // let size = order.size();
        // if !self.size.has_space(size) {
        //     return Err(LimitPoolError::MaxSize(order.order))
        // }
        // self.limit_orders.add_order(order)?;

        Ok(())
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
