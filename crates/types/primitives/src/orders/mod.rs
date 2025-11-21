mod fillstate;
pub use fillstate::*;

use crate::primitive::{OrderId, PoolId, Ray};
mod net_amm_order;
pub use net_amm_order::*;

mod order_set;
pub use order_set::*;

mod cancel_order_request;
pub use cancel_order_request::*;

use crate::sol_bindings::grouped_orders::OrderWithStorageData;
pub type OrderVolume = u128;

use serde::{Deserialize, Serialize};

use crate::sol_bindings::rpc_orders::TopOfBlockOrder;

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderOutcome {
    pub id:      OrderId,
    pub outcome: OrderFillState
}

impl OrderOutcome {
    pub fn is_filled(&self) -> bool {
        self.outcome.is_filled()
    }

    pub fn fill_amount(&self, max: u128) -> u128 {
        match self.outcome {
            OrderFillState::CompleteFill => max,
            OrderFillState::PartialFill(p) => std::cmp::min(max, p),
            _ => 0
        }
    }
}

#[derive(Debug, Clone, Default, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolSolution {
    /// Id of this pool
    pub id:           PoolId,
    /// Uniform clearing price in Ray format
    pub ucp:          Ray,
    /// Winning searcher order to be executed
    pub searcher:     Option<OrderWithStorageData<TopOfBlockOrder>>,
    /// Quantity to be bought or sold from the amm
    pub amm_quantity: Option<NetAmmOrder>,
    /// IDs of limit orders to be executed - it might be easier to just use
    /// hashes here
    pub limit:        Vec<OrderOutcome>,
    /// Any additional reward quantity to be taken out of excess T0 after all
    /// other operations
    pub reward_t0:    u128,
    // fee on user orders.
    pub fee:          u32
}

impl PartialOrd for PoolSolution {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PoolSolution {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.id.cmp(&other.id)
    }
}
