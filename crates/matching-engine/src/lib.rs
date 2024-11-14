#![feature(iter_map_windows)]
use std::collections::{HashMap, HashSet};

use angstrom_types::{
    consensus::PreProposal,
    matching::uniswap::PoolSnapshot,
    orders::PoolSolution,
    primitive::PoolId,
    sol_bindings::grouped_orders::{GroupedVanillaOrder, OrderWithStorageData}
};
use book::OrderBook;
use futures_util::future::BoxFuture;

pub mod book;
pub mod cfmm;
pub mod manager;
pub mod matcher;
pub mod simulation;
pub mod strategy;

pub use manager::MatchingManager;

pub trait MatchingEngineHandle: Send + Sync + Clone + Unpin + 'static {
    fn solve_pools(
        &self,
        preproposals: Vec<PreProposal>,
        pools: HashMap<PoolId, PoolSnapshot>
    ) -> BoxFuture<Result<Vec<PoolSolution>, String>>;
}

pub fn build_book(
    id: PoolId,
    amm: Option<PoolSnapshot>,
    orders: HashSet<OrderWithStorageData<GroupedVanillaOrder>>
) -> OrderBook {
    let (bids, asks) = orders.into_iter().partition(|o| o.is_bid);

    OrderBook::new(id, amm, bids, asks, Some(book::sort::SortStrategy::ByPriceByVolume))
}
