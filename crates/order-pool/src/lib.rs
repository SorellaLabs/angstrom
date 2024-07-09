mod common;
mod config;
mod finalization_pool;
mod inner;
mod limit;
pub mod order_storage;
mod searcher;
mod subscriptions;
mod validator;

use angstrom_types::orders::{OrderConversion, OrderOrigin, PoolOrder};
pub use angstrom_utils::*;
pub use common::Order;
use common::ValidOrder;
pub use config::PoolConfig;
use futures_util::future::BoxFuture;
pub use inner::*;
use tokio_stream::wrappers::ReceiverStream;

#[derive(Debug)]
pub struct OrderSet<Limit: PoolOrder, Searcher: PoolOrder> {
    pub limit:    Vec<BidsAndAsks<Limit>>,
    pub searcher: Vec<ValidOrder<Searcher>>
}

#[derive(Debug)]
pub struct BidsAndAsks<O: PoolOrder> {
    pub bids: Vec<ValidOrder<O>>,
    pub asks: Vec<ValidOrder<O>>
}

type AllOrderFuture<'a, LO, CLO, SO, CSO> = BoxFuture<'a, AllOrders<LO, CLO, SO, CSO>>;
type OrderVecStream<LO, CLO, SO, CSO> = ReceiverStream<Vec<Order<LO, CLO, SO, CSO>>>;
type OrderStream<LO, CLO, SO, CSO> = ReceiverStream<Order<LO, CLO, SO, CSO>>;
#[derive(Debug)]
pub struct AllOrders<
    Limit: PoolOrder,
    Searcher: PoolOrder,
    LimitCompose: PoolOrder,
    SearcherCompose: PoolOrder
> {
    pub vanilla:    OrderSet<Limit, Searcher>,
    pub composable: OrderSet<LimitCompose, SearcherCompose>
}

/// The OrderPool Trait is how other processes can interact with the orderpool
/// asyncly. This allows for requesting data and providing data from different
/// threads efficiently.
pub trait OrderPoolHandle: Send + Sync + Clone + Unpin + 'static {
    /// The transaction type of the limit order pool
    type LimitOrder: PoolOrder;

    /// The transaction type of the searcher order pool
    type SearcherOrder: PoolOrder;

    /// The transaction type of the composable limit order pool
    type ComposableLimitOrder: PoolOrder;

    /// The transaction type of the composable searcher order pool
    type ComposableSearcherOrder: PoolOrder;

    // New order functionality.
    fn new_limit_order(
        &self,
        origin: OrderOrigin,
        order: <Self::LimitOrder as OrderConversion>::Order
    );
    fn new_searcher_order(
        &self,
        origin: OrderOrigin,
        order: <Self::SearcherOrder as OrderConversion>::Order
    );
    fn new_composable_limit_order(
        &self,
        origin: OrderOrigin,
        order: <Self::ComposableLimitOrder as OrderConversion>::Order
    );
    fn new_composable_searcher_order(
        &self,
        origin: OrderOrigin,
        order: <Self::ComposableSearcherOrder as OrderConversion>::Order
    );

    // Queries for fetching all orders. Will be used for quoting
    // and consensus.

    // fetches all vanilla orders
    fn get_all_vanilla_orders(&self) -> BoxFuture<OrderSet<Self::LimitOrder, Self::SearcherOrder>>;
    // fetches all vanilla orders where for each pool the bids and asks overlap plus
    // a buffer on each side
    fn get_all_vanilla_orders_intersection(
        &self,
        buffer: usize
    ) -> BoxFuture<OrderSet<Self::LimitOrder, Self::SearcherOrder>>;

    fn get_all_composable_orders(
        &self
    ) -> BoxFuture<OrderSet<Self::ComposableLimitOrder, Self::ComposableSearcherOrder>>;

    fn get_all_composable_orders_intersection(
        &self,
        buffer: usize
    ) -> BoxFuture<OrderSet<Self::ComposableLimitOrder, Self::ComposableSearcherOrder>>;

    fn get_all_orders(
        &self
    ) -> AllOrderFuture<
        Self::LimitOrder,
        Self::SearcherOrder,
        Self::ComposableLimitOrder,
        Self::ComposableSearcherOrder
    >;

    fn get_all_orders_intersection(
        &self,
        buffer: usize
    ) -> AllOrderFuture<
        Self::LimitOrder,
        Self::SearcherOrder,
        Self::ComposableLimitOrder,
        Self::ComposableSearcherOrder
    >;

    // subscriptions
    fn subscribe_new_orders(
        &self
    ) -> OrderStream<
        Self::LimitOrder,
        Self::ComposableLimitOrder,
        Self::SearcherOrder,
        Self::ComposableSearcherOrder
    >;

    fn subscribe_finalized_orders(
        &self
    ) -> OrderVecStream<
        Self::LimitOrder,
        Self::ComposableLimitOrder,
        Self::SearcherOrder,
        Self::ComposableSearcherOrder
    >;
    fn subscribe_filled_orders(
        &self
    ) -> OrderVecStream<
        Self::LimitOrder,
        Self::ComposableLimitOrder,
        Self::SearcherOrder,
        Self::ComposableSearcherOrder
    >;

    fn subscribe_expired_orders(
        &self
    ) -> OrderVecStream<
        Self::LimitOrder,
        Self::ComposableLimitOrder,
        Self::SearcherOrder,
        Self::ComposableSearcherOrder
    >;
}
