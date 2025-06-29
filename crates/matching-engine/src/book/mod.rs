//! basic book impl so we can benchmark
use std::{iter::Chain, slice::Iter};

use angstrom_types::{
    matching::SqrtPriceX96,
    primitive::PoolId,
    sol_bindings::{
        RawPoolOrder, Ray,
        grouped_orders::{AllOrders, OrderWithStorageData}
    },
    uni_structure::BaselinePoolState
};
use serde::{Deserialize, Serialize};
use uniswap_v3_math::tick_math::{MAX_SQRT_RATIO, MIN_SQRT_RATIO};
use uniswap_v4::uniswap::pool::U256_1;

use self::sort::SortStrategy;

pub type BookOrder = OrderWithStorageData<AllOrders>;

pub mod order;
pub mod sort;

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct OrderBook {
    pub id: PoolId,
    amm:    Option<BaselinePoolState>,
    bids:   Vec<BookOrder>,
    asks:   Vec<BookOrder>
}

impl OrderBook {
    pub fn new(
        id: PoolId,
        amm: Option<BaselinePoolState>,
        mut bids: Vec<BookOrder>,
        mut asks: Vec<BookOrder>,
        sort: Option<SortStrategy>
    ) -> Self {
        // Use our sorting strategy to sort our bids and asks
        let strategy = sort.unwrap_or_default();
        strategy.sort_bids(&mut bids);
        strategy.sort_asks(&mut asks);
        Self { id, amm, bids, asks }
    }

    pub fn id(&self) -> PoolId {
        self.id
    }

    pub fn bids(&self) -> &[BookOrder] {
        &self.bids
    }

    pub fn asks(&self) -> &[BookOrder] {
        &self.asks
    }

    /// Returns a chained iterator that will go over all orders in this book.
    /// Bids first, then asks.
    pub fn all_orders_iter(
        &self
    ) -> Chain<Iter<OrderWithStorageData<AllOrders>>, Iter<OrderWithStorageData<AllOrders>>> {
        self.bids.iter().chain(self.asks.iter())
    }

    pub fn amm(&self) -> Option<&BaselinePoolState> {
        self.amm.as_ref()
    }

    pub fn is_empty_book(&self) -> bool {
        self.bids().is_empty() && self.asks().is_empty()
    }

    pub fn set_amm_if_missing(&mut self, apply: impl FnOnce() -> BaselinePoolState) {
        if self.amm().is_none() {
            self.amm = Some(apply());
        }
    }

    pub fn lowest_clearing_price(&self) -> Ray {
        // because bids need to be ucp <= bid price
        // they don't have a lowest price but rather
        // a max price. Thus we can only use asks to properly set this bound.
        self.asks()
            .iter()
            .map(|ask| ask.limit_price().into())
            .min()
            .unwrap_or_else(|| SqrtPriceX96::from(MIN_SQRT_RATIO + U256_1).into())
    }

    pub fn highest_clearing_price(&self) -> Ray {
        self.bids()
            .iter()
            .map(|bid| Ray::from(bid.limit_price()).inv_ray_round(true))
            .max()
            .unwrap_or_else(|| SqrtPriceX96::from(MAX_SQRT_RATIO - U256_1).into())
    }
}
