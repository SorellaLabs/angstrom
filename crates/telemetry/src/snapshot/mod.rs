use pool::PoolSnapshot;
use serde::{Deserialize, Serialize};

mod builder;
pub mod pool;
pub use builder::SnapshotBuilder;

#[derive(Clone, Serialize, Deserialize)]
pub struct Snapshot {
    block:   u64,
    node_id: u64,
    pools:   Vec<PoolSnapshot>
}
