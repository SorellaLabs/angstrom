use bincode::{Decode, Encode};

use crate::sol_bindings::grouped_orders::AllOrders;

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct OrderBuffer {
    pub excess_orders:  Vec<AllOrders>,
    pub reserve_orders: Vec<AllOrders>
}
