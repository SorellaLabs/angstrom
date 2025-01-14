mod common;
mod config;
mod finalization_pool;
mod limit;
mod order_indexer;
pub mod order_storage;

mod searcher;
mod validator;

use std::future::Future;

use alloy::primitives::{Address, FixedBytes, B256};
use angstrom_types::{
    orders::{CancelOrderRequest, OrderLocation, OrderOrigin, OrderStatus},
    sol_bindings::grouped_orders::{AllOrders, OrderWithStorageData}
};
pub use angstrom_utils::*;
pub use config::PoolConfig;
pub use order_indexer::*;
use tokio_stream::wrappers::BroadcastStream;
use validation::order::OrderPoolNewOrderResult;

#[derive(Debug, Clone)]
pub enum PoolManagerUpdate {
    NewOrder(OrderWithStorageData<AllOrders>),
    FilledOrder(u64, OrderWithStorageData<AllOrders>),
    UnfilledOrders(OrderWithStorageData<AllOrders>),
    CancelledOrder { user: Address, pool_id: FixedBytes<32>, order_hash: B256 }
}

/// The OrderPool Trait is how other processes can interact with the orderpool
/// asyncly. This allows for requesting data and providing data from different
/// threads efficiently.
pub trait OrderPoolHandle: Send + Sync + Clone + Unpin + 'static {
    fn new_order(
        &self,
        origin: OrderOrigin,
        order: AllOrders
    ) -> impl Future<Output = OrderPoolNewOrderResult> + Send;

    fn subscribe_orders(&self) -> BroadcastStream<PoolManagerUpdate>;

    fn pending_orders(&self, sender: Address) -> impl Future<Output = Vec<AllOrders>> + Send;

    fn cancel_order(&self, req: CancelOrderRequest) -> impl Future<Output = bool> + Send;

    fn fetch_orders_from_pool(
        &self,
        pool_id: FixedBytes<32>,
        location: OrderLocation
    ) -> impl Future<Output = Vec<AllOrders>> + Send;

    fn fetch_order_status(
        &self,
        order_hash: B256
    ) -> impl Future<Output = Option<OrderStatus>> + Send;
}
