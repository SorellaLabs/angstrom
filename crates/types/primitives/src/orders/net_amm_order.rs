use alloy_primitives::I256;
use serde::{Deserialize, Serialize};

use crate::primitive::Direction;

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetAmmOrder {
    /// A NetAmmOrder that is Buying will be purchasing T0 from the AMM
    Buy(u128, u128),
    /// A NetAmmOrder that is Selling will be selling T0 to the AMM
    Sell(u128, u128)
}

impl Default for NetAmmOrder {
    fn default() -> Self {
        Self::Buy(0, 0)
    }
}

impl NetAmmOrder {
    pub fn new(direction: Direction) -> Self {
        match direction {
            Direction::BuyingT0 => Self::Buy(0, 0),
            Direction::SellingT0 => Self::Sell(0, 0)
        }
    }

    pub fn right_direction(&self, direction: Direction) -> bool {
        match direction {
            Direction::BuyingT0 => matches!(self, Self::Sell(_, _)),
            Direction::SellingT0 => matches!(self, Self::Buy(_, _))
        }
    }

    pub fn add_quantity(&mut self, quantity: u128, cost: u128) {
        let (my_quantity, my_cost) = match self {
            Self::Buy(q, c) => (q, c),
            Self::Sell(q, c) => (q, c)
        };
        *my_cost += cost;
        *my_quantity += quantity;
    }

    pub fn remove_quantity(&mut self, quantity: u128, cost: u128) {
        let (my_quantity, my_cost) = match self {
            Self::Buy(q, c) => (q, c),
            Self::Sell(q, c) => (q, c)
        };
        *my_cost -= cost;
        *my_quantity -= quantity;
    }

    pub fn get_directions(&self) -> (u128, u128) {
        match self {
            Self::Buy(amount_out, amount_in) => (*amount_in, *amount_out),
            Self::Sell(amount_in, amount_out) => (*amount_in, *amount_out)
        }
    }

    /// Gets the net AMM order as a signed quantity of T0.  The quantity is
    /// positive if we are purchasing T0 from the AMM and negative if we are
    /// selling T0 into the AMM
    pub fn get_t0_signed(&self) -> I256 {
        tracing::trace!(net_amm_order = ?self, "Processing net AMM order into t0signed");
        match self {
            Self::Buy(t0, _) => I256::unchecked_from(*t0),
            Self::Sell(t0, _) => I256::unchecked_from(*t0).saturating_neg()
        }
    }

    pub fn amount_in(&self) -> u128 {
        self.get_directions().0
    }

    pub fn amount_out(&self) -> u128 {
        self.get_directions().1
    }

    pub fn to_order_tuple(&self, t0_idx: u16, t1_idx: u16) -> (u16, u16, u128, u128) {
        match self {
            NetAmmOrder::Buy(q, c) => (t1_idx, t0_idx, *c, *q),
            NetAmmOrder::Sell(q, c) => (t0_idx, t1_idx, *q, *c)
        }
    }

    pub fn is_bid(&self) -> bool {
        matches!(self, Self::Buy(_, _))
    }
}
