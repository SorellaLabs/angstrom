mod common;
mod config;
mod finalization_pool;
mod limit;
mod order_indexer;
pub mod order_storage;

mod searcher;
mod validator;

use angstrom_types::{
    orders::{OrderOrigin, OrderSet},
    sol_bindings::{
        grouped_orders::{AllOrders, GroupedVanillaOrder},
        sol::TopOfBlockOrder
    }
};
pub use angstrom_utils::*;
pub use config::PoolConfig;
use futures_util::future::BoxFuture;
pub use order_indexer::*;

/// The OrderPool Trait is how other processes can interact with the orderpool
/// asyncly. This allows for requesting data and providing data from different
/// threads efficiently.
pub trait OrderPoolHandle: Send + Sync + Clone + Unpin + 'static {
    fn new_order(&self, origin: OrderOrigin, order: AllOrders);
    // Queries for fetching all orders. Will be used for quoting
    // and consensus.

    // fetches all vanilla orders

    fn get_all_vanilla_orders(&self) -> BoxFuture<OrderSet<GroupedVanillaOrder, TopOfBlockOrder>>;

    // fn get_all_composable_orders(
    //     &self
    // ) -> BoxFuture<OrderSet<Self::ComposableLimitOrder,
    // Self::ComposableSearcherOrder>>;

    // fn get_all_orders(
    //     &self
    // ) -> AllOrderFuture<
    //     Self::LimitOrder,
    //     Self::SearcherOrder,
    //     Self::ComposableLimitOrder,
    //     Self::ComposableSearcherOrder
    // >;
}
