mod common;
mod config;
mod finalization_pool;
mod limit;
mod order_indexer;
pub mod order_storage;
mod order_subscribers;
pub mod order_tracker;
pub mod telemetry;

mod searcher;
mod validator;

use std::future::Future;

use alloy::primitives::{Address, B256, FixedBytes};
use angstrom_types::{
    orders::{CancelOrderRequest, OrderLocation, OrderOrigin, OrderStatus},
    primitive::OrderValidationError,
    sol_bindings::grouped_orders::{AllOrders, OrderWithStorageData}
};
pub use angstrom_utils::*;
pub use config::PoolConfig;
pub use order_indexer::*;
use tokio_stream::wrappers::BroadcastStream;

#[derive(Debug, Clone)]
pub enum PoolManagerUpdate {
    NewOrder(OrderWithStorageData<AllOrders>),
    FilledOrder(u64, OrderWithStorageData<AllOrders>),
    UnfilledOrders(OrderWithStorageData<AllOrders>),
    CancelledOrder {
        is_tob:     bool,
        user:       Address,
        pool_id:    FixedBytes<32>,
        order_hash: B256
    },
    ExpiredOrder(OrderWithStorageData<AllOrders>)
}
impl PoolManagerUpdate {
    pub fn order_id(&self) -> B256 {
        match self {
            Self::NewOrder(o) => o.order_id.hash,
            Self::FilledOrder(_, o) => o.order_id.hash,
            Self::UnfilledOrders(o) => o.order_id.hash,
            Self::CancelledOrder { order_hash, .. } => *order_hash,
            Self::ExpiredOrder(o) => o.order_id.hash
        }
    }

    pub fn last_notification_for_order(&self) -> bool {
        matches!(
            self,
            PoolManagerUpdate::FilledOrder(..)
                | PoolManagerUpdate::ExpiredOrder(..)
                | PoolManagerUpdate::CancelledOrder { .. }
        )
    }
}

/// The OrderPool Trait is how other processes can interact with the orderpool
/// asyncly. This allows for requesting data and providing data from different
/// threads efficiently.
pub trait OrderPoolHandle: Send + Sync + Clone + Unpin + 'static {
    fn new_order(
        &self,
        origin: OrderOrigin,
        order: AllOrders
    ) -> impl Future<Output = Result<FixedBytes<32>, OrderValidationError>> + Send;

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
