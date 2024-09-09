use serde::{Deserialize, Serialize};

use crate::sol_bindings::grouped_orders::AllOrders;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderBuffer {
    pub excess_orders:  Vec<AllOrders>,
    pub reserve_orders: Vec<AllOrders>
}
