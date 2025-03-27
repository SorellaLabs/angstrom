pub mod quoting;
pub mod subscriptions;
use alloy_primitives::FixedBytes;
use angstrom_types::sol_bindings::grouped_orders::AllOrders;
pub use quoting::*;
use serde::{Deserialize, Serialize};
pub use subscriptions::*;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PendingOrder {
    /// the order id
    pub order_id: FixedBytes<32>,
    pub order:    AllOrders
}
