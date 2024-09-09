mod fillstate;
mod origin;
use alloy_primitives::U256;
pub mod orderpool;

pub use fillstate::*;
pub use orderpool::*;
pub use origin::*;
use serde::{Deserialize, Serialize};

pub type BookID = u128;
pub type OrderID = u128;
pub type OrderVolume = U256;
pub type OrderPrice = MatchingPrice;

// mod pooled;
// pub use pooled::*;
use crate::{
    matching::MatchingPrice,
    primitive::PoolId,
    sol_bindings::{grouped_orders::OrderWithStorageData, sol::TopOfBlockOrder}
};

#[derive(Debug)]
pub struct OrderSet<Limit, Searcher> {
    pub limit:    Vec<OrderWithStorageData<Limit>>,
    pub searcher: Vec<OrderWithStorageData<Searcher>>
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetAmmOrder {
    Buy(U256),
    Sell(U256)
}

impl Default for NetAmmOrder {
    fn default() -> Self {
        Self::Buy(U256::ZERO)
    }
}

impl NetAmmOrder {
    pub fn new(is_bid: bool) -> Self {
        if is_bid {
            Self::Sell(U256::ZERO)
        } else {
            Self::Buy(U256::ZERO)
        }
    }

    pub fn add_quantity(&mut self, quantity: U256) {
        let my_quantity = match self {
            Self::Buy(q) => q,
            Self::Sell(q) => q
        };
        *my_quantity += quantity
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderOutcome {
    pub id:      OrderId,
    pub outcome: OrderFillState
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolSolution {
    /// Id of this pool
    pub id:           PoolId,
    /// Winning searcher order to be executed
    pub searcher:     Option<OrderWithStorageData<TopOfBlockOrder>>,
    /// Quantity to be bought or sold from the amm
    pub amm_quantity: Option<NetAmmOrder>,
    /// IDs of limit orders to be executed - it might be easier to just use
    /// hashes here
    pub limit:        Vec<OrderOutcome>
}
