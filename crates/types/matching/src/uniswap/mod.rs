mod donationresult;
mod flags;
mod liqrange;
mod poolprice;
mod poolpricevec;
mod poolsnapshot;

pub use donationresult::DonationResult;
pub use flags::*;
pub use liqrange::{LiqRange, LiqRangeRef};
pub use poolprice::PoolPrice;
pub use poolpricevec::PoolPriceVec;
pub use poolsnapshot::PoolSnapshot;
use serde::{Deserialize, Serialize};

pub type Tick = i32;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TickInfo {
    pub liquidity_gross: u128,
    pub liquidity_net:   i128,
    pub initialized:     bool
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub enum Quantity {
    Token0(u128),
    Token1(u128)
}

impl Quantity {
    pub fn magnitude(&self) -> u128 {
        match self {
            Self::Token0(q) => *q,
            Self::Token1(q) => *q
        }
    }

    /// The direction of the swap if this quantity is used as input
    pub fn as_input(&self) -> Direction {
        match self {
            Self::Token0(_) => Direction::SellingT0,
            Self::Token1(_) => Direction::BuyingT0
        }
    }

    /// The direction of the swap if this quantity is used as output
    pub fn as_output(&self) -> Direction {
        match self {
            Self::Token0(_) => Direction::BuyingT0,
            Self::Token1(_) => Direction::SellingT0
        }
    }
}

#[derive(Copy, Clone, Debug)]
/// Direction is used from the perspective of the operation and not from the
/// perspective of the Uniswap pool itself.  In other words, "buying T0" means
/// putting T1 into the pool to get T0 out, which will decrease the overall
/// amount of T0 left in the pool.
/// TODO: CONSISTENCY CHECK FOR USAGE
pub enum Direction {
    /// When the contract is buying T0 from the pool, the price will go up and
    /// the tick number will increase
    BuyingT0,
    /// When the contract is selling T0 to the pool, the price will go down and
    /// the tick number will decrease
    SellingT0
}

impl Direction {
    /// Returns the Direction associated with an order type.  `is_bid` is true
    /// if the order or AMM interaction we are referencing is acting as a bid
    /// (Offering to purchase T0 from the contract) or false if the order or AMM
    /// interaction we are referencing is acting as an ask (Offering to sell T0
    /// to the contract)
    pub fn from_is_bid(is_bid: bool) -> Self {
        match is_bid {
            // If the pool is acting as a bid, the contract is selling T0 to the pool
            true => Self::SellingT0,
            // If the pool is acting as an ask, the contract is buying T0 from the pool
            false => Self::BuyingT0
        }
    }

    /// Returns `true` if this direction is the proper direction to represent a
    /// bid-type interaction from the contract's perspective (An interaction
    /// that is purchasing T0 from the contract)
    pub fn is_bid(&self) -> bool {
        // if we are selling t0 we are buying t1 which is a ask
        matches!(self, Self::BuyingT0)
    }

    pub fn is_ask(&self) -> bool {
        matches!(self, Self::SellingT0)
    }

    /// Determine the direction of sale from a start and end price
    pub fn from_prices<P: Ord>(start: P, end: P) -> Self {
        match start.cmp(&end) {
            std::cmp::Ordering::Less => Self::BuyingT0,
            _ => Self::SellingT0
        }
    }

    /// Returns true if the given quantity is on the input side of this
    /// direction
    pub fn is_input(&self, q: &Quantity) -> bool {
        matches!(
            (self, q),
            (Self::BuyingT0, Quantity::Token1(_)) | (Self::SellingT0, Quantity::Token0(_))
        )
    }

    /// Given our transaction direction, turns (amount_in, amount_out) into
    /// (token0, token1) for use when working with a uniswap pool
    pub fn sort_tokens<T>(&self, amount_in: T, amount_out: T) -> (T, T) {
        match self {
            Self::BuyingT0 => (amount_out, amount_in),
            Self::SellingT0 => (amount_in, amount_out)
        }
    }

    /// Given our transaction direction turns (q_t0, q_t1) into (amount_in,
    /// amount_out)
    pub fn sort_amounts<T>(&self, token0: T, token1: T) -> (T, T) {
        match self {
            Self::BuyingT0 => (token1, token0),
            Self::SellingT0 => (token0, token1)
        }
    }
}

/*
A Uniswap pool is a relation between Token0 and Token1.  The price is defined as Token1/Token0.  We might use terms like
'buy' and 'sell' in here, those terms are in the context of Token0 where "buy" is input Token1 to get Token0
out and "sell" is input Token0 to get Token1 out
 */
