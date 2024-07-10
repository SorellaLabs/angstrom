mod common;
mod config;
mod finalization_pool;
mod limit;
mod order_indexer;
pub mod order_storage;

mod searcher;
mod validator;

use angstrom_types::{orders::OrderOrigin, sol_bindings::grouped_orders::AllOrders};
pub use angstrom_utils::*;
pub use config::PoolConfig;
pub use order_indexer::*;

/// The OrderPool Trait is how other processes can interact with the orderpool
/// asyncly. This allows for requesting data and providing data from different
/// threads efficiently.
pub trait OrderPoolHandle: Send + Sync + Clone + Unpin + 'static {
    fn new_order(&self, origin: OrderOrigin, order: AllOrders);
}
