use std::collections::{HashMap, HashSet};

use alloy_primitives::Address;
use angstrom_types::{
    contract_payloads::angstrom::BundleGasDetails,
    orders::PoolSolution,
    primitive::PoolId,
    sol_bindings::{
        RawPoolOrder, grouped_orders::OrderWithStorageData, rpc_orders::TopOfBlockOrder
    },
    uni_structure::BaselinePoolState
};
use book::{BookOrder, OrderBook};
use futures_util::future::BoxFuture;

pub mod book;
pub mod manager;
pub mod matcher;
pub mod simulation;
pub mod strategy;

pub use manager::MatchingManager;

pub trait MatchingEngineHandle: Send + Sync + Clone + Unpin + 'static {
    fn solve_pools(
        &self,
        limit: Vec<BookOrder>,
        searcher: Vec<OrderWithStorageData<TopOfBlockOrder>>,
        pools: HashMap<PoolId, (Address, Address, BaselinePoolState, u16)>
    ) -> BoxFuture<eyre::Result<(Vec<PoolSolution>, BundleGasDetails)>>;
}

pub fn build_book(
    id: PoolId,
    amm: Option<BaselinePoolState>,
    orders: HashSet<BookOrder>
) -> OrderBook {
    let (mut bids, mut asks): (Vec<BookOrder>, Vec<BookOrder>) =
        orders.into_iter().partition(|o| o.is_bid);

    // assert bids decreasing and asks increasing
    bids.sort_by_key(|b| std::cmp::Reverse(b.limit_price()));
    asks.sort_by_key(|a| a.limit_price());

    OrderBook::new(id, amm, bids, asks, Some(book::sort::SortStrategy::ByPriceByVolume))
}
