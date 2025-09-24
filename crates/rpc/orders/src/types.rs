use alloy_primitives::FixedBytes;
use angstrom_types::{primitive::OrderValidationError, sol_bindings::grouped_orders::AllOrders};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(
    Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, EnumIter,
)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub enum OrderSubscriptionKind {
    /// Any new orders
    NewOrders,
    /// Any new filled orders
    FilledOrders,
    /// Any new reorged orders
    UnfilledOrders,
    /// Any new cancelled orders
    CancelledOrders,
    /// Orders that expire.
    ExpiredOrders
}

impl OrderSubscriptionKind {
    pub fn all() -> OrderSubscriptionKindIter {
        Self::iter()
    }
}

#[derive(
    Debug, Default, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub enum OrderSubscriptionFilter {
    /// only returns subscription updates on a singluar pair
    ByPair(FixedBytes<32>),
    /// only returns subscription updates related to a address
    ByAddress(Address),
    /// only TOB orders
    OnlyTOB,
    /// only book orders
    OnlyBook,
    /// returns all subscription updates
    #[default]
    None
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub enum OrderSubscriptionResult {
    NewOrder(AllOrders),
    FilledOrder(u64, AllOrders),
    UnfilledOrder(AllOrders),
    CancelledOrder(B256),
    ExpiredOrder(AllOrders)
}

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
