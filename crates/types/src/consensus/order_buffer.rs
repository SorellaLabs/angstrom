use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};

use crate::{rpc::SignedLimitOrder, sol_bindings::grouped_orders::AllOrders};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Encode, Decode)]
pub struct OrderBuffer {
    pub excess_orders:  Vec<AllOrders>,
    pub reserve_orders: Vec<AllOrders>
}
