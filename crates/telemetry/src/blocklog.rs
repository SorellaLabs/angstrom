use std::collections::HashMap;

use angstrom_types::{primitive::PoolId, uni_structure::BaselinePoolState};
use serde::{Deserialize, Serialize};

use crate::TelemetryMessage;

#[derive(Serialize, Deserialize)]
pub struct BlockLog {
    blocknum:       u64,
    pool_snapshots: Option<HashMap<PoolId, BaselinePoolState>>,
    events:         Vec<TelemetryMessage>
}

impl BlockLog {
    pub fn new(blocknum: u64) -> Self {
        Self { blocknum, pool_snapshots: None, events: Vec::new() }
    }

    pub fn blocknum(&self) -> u64 {
        self.blocknum
    }

    pub fn set_pool_snapshots(&mut self, pool_snapshots: HashMap<PoolId, BaselinePoolState>) {
        self.pool_snapshots = Some(pool_snapshots);
    }

    pub fn add_event(&mut self, event: TelemetryMessage) {
        self.events.push(event)
    }
}
