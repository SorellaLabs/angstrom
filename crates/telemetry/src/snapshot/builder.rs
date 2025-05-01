use super::{Snapshot, pool::PoolSnapshot};

#[derive(Default, Debug)]
pub struct SnapshotBuilder {
    block:   Option<u64>,
    node_id: Option<u64>,
    pools:   Option<Vec<PoolSnapshot>>
}

impl SnapshotBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn block(self, b: u64) -> Self {
        Self { block: Some(b), ..self }
    }

    pub fn node_id(self, n: u64) -> Self {
        Self { node_id: Some(n), ..self }
    }

    pub fn pools(self, v: Vec<PoolSnapshot>) -> Self {
        Self { pools: Some(v), ..self }
    }

    pub fn build(self) -> Snapshot {
        Snapshot {
            block:   self.block.unwrap_or_default(),
            node_id: self.node_id.unwrap_or_default(),
            pools:   self.pools.unwrap_or_default()
        }
    }
}
