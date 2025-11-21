pub mod quoting;
pub mod subscriptions;
use alloy_primitives::FixedBytes;
use angstrom_types::{primitive::OrderValidationError, sol_bindings::grouped_orders::AllOrders};
pub use quoting::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub use subscriptions::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct PendingOrder {
    /// the order id
    pub order_id: FixedBytes<32>,
    pub order:    AllOrders
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct CallResult {
    pub is_success: bool,
    pub data:       Value,
    /// only will show up on error
    pub msg:        String
}

impl CallResult {
    pub fn is_ok(&self) -> bool {
        self.is_success
    }

    pub fn from_success<T>(return_value: T) -> Self
    where
        T: Serialize
    {
        Self {
            is_success: true,
            data:       serde_json::to_value(return_value).unwrap(),
            msg:        String::default()
        }
    }
}

impl From<OrderValidationError> for CallResult {
    fn from(value: OrderValidationError) -> Self {
        let msg = value.to_string();

        let data = if let OrderValidationError::StateError(state) = &value {
            serde_json::to_value(state).unwrap()
        } else {
            serde_json::to_value(value).unwrap()
        };

        Self { is_success: false, data, msg }
    }
}
