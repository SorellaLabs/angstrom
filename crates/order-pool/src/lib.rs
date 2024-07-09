mod common;
mod config;
mod finalization_pool;
mod limit;
mod order_sorter;
pub mod order_storage;

mod searcher;
mod subscriptions;
mod validator;

use angstrom_types::orders::{OrderOrigin, PoolOrder};
pub use angstrom_utils::*;
pub use common::Order;
pub use config::PoolConfig;
pub use order_sorter::*;
use sol_bindings::grouped_orders::AllOrders;

/// The OrderPool Trait is how other processes can interact with the orderpool
/// asyncly. This allows for requesting data and providing data from different
/// threads efficiently.
pub trait OrderPoolHandle: Send + Sync + Clone + Unpin + 'static {
    fn new_order(&self, origin: OrderOrigin, order: AllOrders);
}
