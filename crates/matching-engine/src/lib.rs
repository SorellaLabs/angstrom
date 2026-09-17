use std::collections::{HashMap, HashSet};

use alloy_primitives::{Address, B256};
use angstrom_types::{
    contract_payloads::{angstrom::BundleGasDetails, protocol_fees::DonationSplits},
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

use crate::manager::MatchingEngineError;

pub trait MatchingEngineHandle: Send + Sync + Clone + Unpin + 'static {
    /// `parent_hash` is the H the round is building on. It reaches bundle
    /// simulation unchanged, so the gas the solutions are priced with comes
    /// from the same state the bundle was built against.
    ///
    /// `splits` is the rates from that round's one read of the configuration,
    /// handed in rather than read here, so the bundle this prices and the
    /// bundle finally constructed are built from the same rates. Only the
    /// rates: the read's own block identity is not this code's business,
    /// and carrying it would put a second, unrelated hash beside
    /// `parent_hash`.
    fn solve_pools(
        &self,
        limit: Vec<BookOrder>,
        searcher: Vec<OrderWithStorageData<TopOfBlockOrder>>,
        pools: HashMap<PoolId, (Address, Address, BaselinePoolState, u16)>,
        parent_hash: B256,
        splits: DonationSplits
    ) -> BoxFuture<'_, Result<(Vec<PoolSolution>, BundleGasDetails), MatchingEngineError>>;
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

    OrderBook::new(id, amm, bids, asks, Some(book::sort::SortStrategy::PricePartialVolume))
}
