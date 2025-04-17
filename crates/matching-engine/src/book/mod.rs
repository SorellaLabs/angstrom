//! basic book impl so we can benchmark
use std::{io::Write, iter::Chain, slice::Iter, time::UNIX_EPOCH};

use alloy_primitives::U256;
use angstrom_types::{
    matching::{SqrtPriceX96, uniswap::PoolSnapshot},
    primitive::PoolId,
    sol_bindings::{
        Ray,
        grouped_orders::{GroupedVanillaOrder, OrderWithStorageData}
    },
    uni_structure::BaselinePoolState
};
use serde::{Deserialize, Serialize};
use uniswap_v3_math::tick_math::{MAX_SQRT_RATIO, MIN_SQRT_RATIO};

use self::sort::SortStrategy;

pub type BookOrder = OrderWithStorageData<GroupedVanillaOrder>;

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
    ) -> Chain<
        Iter<OrderWithStorageData<GroupedVanillaOrder>>,
        Iter<OrderWithStorageData<GroupedVanillaOrder>>
    > {
        self.bids.iter().chain(self.asks.iter())
    }

    pub fn amm(&self) -> Option<&BaselinePoolState> {
        self.amm.as_ref()
    }

    pub fn lowest_clearing_price(&self) -> Ray {
        self.bids()
            .iter()
            .map(|bid| bid.bid_price() / U256::from(2))
            .chain(self.asks().iter().map(|ask| ask.price() / U256::from(2)))
            .min()
            .unwrap_or_else(|| SqrtPriceX96::from(MIN_SQRT_RATIO).into())
    }

    pub fn highest_clearing_price(&self) -> Ray {
        self.bids()
            .iter()
            .map(|bid| bid.bid_price() * U256::from(2))
            .chain(self.asks().iter().map(|ask| ask.price() * U256::from(2)))
            .max()
            .unwrap_or_else(|| SqrtPriceX96::from(MAX_SQRT_RATIO).into())
    }
}
