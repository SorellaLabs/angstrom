use pool::PoolSnapshot;
use serde::{Deserialize, Serialize};

pub mod pool;

#[derive(Clone, Serialize, Deserialize)]
pub struct Snapshot {
    block: u64,
    node:  u64,
    pools: Vec<PoolSnapshot>
}
