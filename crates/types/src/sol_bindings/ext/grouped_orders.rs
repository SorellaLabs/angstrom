use crate::{
    orders::OrderPriorityData,
    sol_bindings::user_types::{FlashOrder, StandingOrder, TopOfBlockOrder}
};

#[derive(Debug)]
pub enum AllOrders {
    Partial(StandingOrder),
    KillOrFill(FlashOrder),
    TOB(TopOfBlockOrder)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderWithId<Order> {
    pub order:         Order,
    pub id:            u64,
    pub priority_data: OrderPriorityData
}

pub enum GroupedVanillaOrders {
    Partial(StandingOrder),
    KillOrFill(FlashOrder)
}
