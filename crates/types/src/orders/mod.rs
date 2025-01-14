mod fillstate;
mod origin;
use alloy::{
    primitives::{keccak256, Address, FixedBytes, PrimitiveSignature, B256, U256},
    sol_types::SolValue
};
pub mod orderpool;

pub use fillstate::*;
pub use orderpool::*;
pub use origin::*;
use serde::{Deserialize, Serialize};

pub type BookID = u128;
pub type OrderID = u128;
pub type OrderVolume = u128;
pub type OrderPrice = MatchingPrice;

use crate::{
    matching::{MatchingPrice, Ray},
    primitive::PoolId,
    sol_bindings::{grouped_orders::OrderWithStorageData, rpc_orders::TopOfBlockOrder}
};

#[derive(Debug)]
pub struct OrderSet<Limit, Searcher> {
    pub limit:    Vec<OrderWithStorageData<Limit>>,
    pub searcher: Vec<OrderWithStorageData<Searcher>>
}

impl<Limit, Searcher> OrderSet<Limit, Searcher> {
    pub fn total_orders(&self) -> usize {
        self.limit.len() + self.searcher.len()
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetAmmOrder {
    Buy(U256, U256),
    Sell(U256, U256)
}

impl Default for NetAmmOrder {
    fn default() -> Self {
        Self::Buy(U256::ZERO, U256::ZERO)
    }
}

impl NetAmmOrder {
    pub fn new(is_bid: bool) -> Self {
        if is_bid {
            Self::Sell(U256::ZERO, U256::ZERO)
        } else {
            Self::Buy(U256::ZERO, U256::ZERO)
        }
    }

    pub fn add_quantity(&mut self, quantity: U256, cost: U256) {
        let (my_quantity, my_cost) = match self {
            Self::Buy(q, c) => (q, c),
            Self::Sell(q, c) => (q, c)
        };
        *my_cost += cost;
        *my_quantity += quantity;
    }

    fn get_directions(&self) -> (U256, U256) {
        match self {
            Self::Buy(amount_out, amount_in) => (*amount_in, *amount_out),
            Self::Sell(amount_in, amount_out) => (*amount_in, *amount_out)
        }
    }

    pub fn amount_in(&self) -> U256 {
        self.get_directions().0
    }

    pub fn amount_out(&self) -> U256 {
        self.get_directions().1
    }

    pub fn to_order_tuple(&self, t0_idx: u16, t1_idx: u16) -> (u16, u16, u128, u128) {
        match self {
            NetAmmOrder::Buy(q, c) => (t1_idx, t0_idx, c.to(), q.to()),
            NetAmmOrder::Sell(q, c) => (t0_idx, t1_idx, q.to(), c.to())
        }
    }
}

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
    pub limit:        Vec<OrderOutcome>
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

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CancelOrderRequest {
    pub signature:    PrimitiveSignature,
    // if there's no salt to make this a unique signing hash. One can just
    // copy the signature of the order and id and it will verify
    pub user_address: Address,
    pub order_id:     B256
}

impl CancelOrderRequest {
    fn signing_payload(&self) -> FixedBytes<32> {
        keccak256((self.user_address, self.order_id).abi_encode())
    }

    pub fn is_valid(&self) -> bool {
        let hash = self.signing_payload();
        let Ok(sender) = self.signature.recover_address_from_prehash(&hash) else { return false };

        sender == self.user_address
    }
}
