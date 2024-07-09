use crate::user_types::{FlashOrder, StandingOrder, TopOfBlockOrder};

#[derive(Debug)]
pub enum AllOrders {
    Partial(StandingOrder),
    KillOrFill(FlashOrder),
    TOB(TopOfBlockOrder)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OrderWithId<Order> {
    pub order: Order,
    pub id:    u64
}

pub enum GroupedVanillaOrders {
    Partial(StandingOrder),
    KillOrFill(FlashOrder)
}
