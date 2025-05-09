use std::{collections::HashMap, fmt::Debug};

use angstrom_types::{
    orders::{CancelOrderRequest, OrderOrigin},
    primitive::PoolId,
    sol_bindings::grouped_orders::AllOrders,
    uni_structure::BaselinePoolState
};
use tokio::sync::mpsc::UnboundedSender;

use crate::TelemetryMessage;

pub trait TelemetryHandle: Send + Sync + Clone + Debug + Unpin + 'static {
    fn pools(&self, blocknum: u64, pool_snapshots: HashMap<PoolId, BaselinePoolState>);
    fn new_order(&self, blocknum: u64, origin: OrderOrigin, order: AllOrders);
    fn cancel_order(&self, blocknum: u64, cancel: CancelOrderRequest);
}

#[derive(Clone, Debug)]
pub struct TelemetryClient {
    tx: UnboundedSender<TelemetryMessage>
}

impl TelemetryClient {
    pub fn new(tx: UnboundedSender<TelemetryMessage>) -> Self {
        Self { tx }
    }
}

impl TelemetryHandle for TelemetryClient {
    fn pools(&self, blocknum: u64, pool_snapshots: HashMap<PoolId, BaselinePoolState>) {
        let _ = self
            .tx
            .send(TelemetryMessage::NewBlock { blocknum, pool_snapshots });
    }

    fn new_order(&self, blocknum: u64, origin: OrderOrigin, order: AllOrders) {
        let _ = self
            .tx
            .send(TelemetryMessage::NewOrder { blocknum, origin, order });
    }

    fn cancel_order(&self, blocknum: u64, cancel: CancelOrderRequest) {
        let _ = self
            .tx
            .send(TelemetryMessage::CancelOrder { blocknum, cancel });
    }
}
