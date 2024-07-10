use crate::{
    orders::OrderPriorityData,
    primitive::PoolId,
    sol_bindings::user_types::{FlashOrder, StandingOrder, TopOfBlockOrder}
};

#[derive(Debug)]
pub enum AllOrders {
    Partial(StandingOrder),
    KillOrFill(FlashOrder),
    TOB(TopOfBlockOrder)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderWithStorageData<Order> {
    /// raw order
    pub order:              Order,
    /// internal order id
    pub id:                 u64,
    /// the raw data needed for indexing the data
    pub priority_data:      OrderPriorityData,
    /// the pool this order belongs to
    pub pool_id:            PoolId,
    /// wether the order is waiting for approvals / proper balances
    pub is_currently_valid: bool,
    /// what side of the book does this order lay on
    pub is_bid:             bool
}

impl<Order> OrderWithStorageData<Order> {
    pub fn size(&self) -> usize {
        std::mem::size_of::<Order>()
    }
}

#[derive(Debug)]
pub enum GroupedVanillaOrder {
    Partial(StandingOrder),
    KillOrFill(FlashOrder)
}

#[derive(Debug)]
pub enum GroupedComposableOrder {
    Partial(StandingOrder),
    KillOrFill(FlashOrder)
}
