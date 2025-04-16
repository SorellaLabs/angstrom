use std::collections::{HashMap, HashSet};

use alloy_primitives::Address;
use angstrom_types::{
    contract_payloads::angstrom::BundleGasDetails,
    matching::uniswap::PoolSnapshot,
    orders::PoolSolution,
    primitive::PoolId,
    sol_bindings::{
        RawPoolOrder, grouped_orders::OrderWithStorageData, rpc_orders::TopOfBlockOrder
    }
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
        pools: HashMap<PoolId, (Address, Address, PoolSnapshot, u16)>
    ) -> BoxFuture<eyre::Result<(Vec<PoolSolution>, BundleGasDetails)>>;
}

pub fn build_book(id: PoolId, amm: Option<PoolSnapshot>, orders: HashSet<BookOrder>) -> OrderBook {
    let (bids, asks): (Vec<BookOrder>, Vec<BookOrder>) = orders.into_iter().partition(|o| o.is_bid);

    OrderBook::new(id, amm, bids, asks, Some(book::sort::SortStrategy::PricePartialVolume))
}
