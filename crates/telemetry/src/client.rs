use std::collections::HashMap;

use angstrom_types::{primitive::PoolId, uni_structure::BaselinePoolState};
use tokio::sync::mpsc::UnboundedSender;

use crate::TelemetryMessage;

pub trait TelemetryHandle {
    fn pools(&self, blocknum: u64, pool_snapshots: HashMap<PoolId, BaselinePoolState>);
}

pub struct TelemetryClient {
    tx: UnboundedSender<TelemetryMessage>
}

impl TelemetryHandle for TelemetryClient {
    fn pools(&self, blocknum: u64, pool_snapshots: HashMap<PoolId, BaselinePoolState>) {
        let _ = self
            .tx
            .send(TelemetryMessage::NewBlock { blocknum, pool_snapshots });
    }
}
