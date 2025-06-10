use std::{
    collections::{HashMap, VecDeque},
    fmt::Debug
};

use angstrom_types::{
    consensus::{ConsensusRoundName, StromConsensusEvent},
    contract_bindings::angstrom::Angstrom::PoolKey,
    orders::{CancelOrderRequest, OrderOrigin},
    pair_with_price::PairsWithPrice,
    primitive::PoolId,
    sol_bindings::grouped_orders::AllOrders,
    uni_structure::BaselinePoolState
};
use tokio::sync::mpsc::UnboundedSender;

use crate::TelemetryMessage;

pub trait TelemetryHandle: Send + Sync + Clone + Debug + Unpin + 'static {
    fn pools(
        &self,
        blocknum: u64,
        pool_keys: Vec<PoolKey>,
        pool_snapshots: HashMap<PoolId, BaselinePoolState>
    );
    fn new_order(&self, blocknum: u64, origin: OrderOrigin, order: AllOrders);
    fn cancel_order(&self, blocknum: u64, cancel: CancelOrderRequest);
    fn consensus_event(&self, event: StromConsensusEvent);
    fn consensus_state(&self, blocknum: u64, state: ConsensusRoundName);
    fn gas_price_snapshot(
        &self,
        blocknum: u64,
        snapshot: (HashMap<PoolId, VecDeque<PairsWithPrice>>, u128)
    );
    fn error(&self, blocknum: u64, message: String);
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
    fn pools(
        &self,
        blocknum: u64,
        pool_keys: Vec<PoolKey>,
        pool_snapshots: HashMap<PoolId, BaselinePoolState>
    ) {
        let _ = self
            .tx
            .send(TelemetryMessage::NewBlock { blocknum, pool_keys, pool_snapshots });
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

    fn consensus_event(&self, event: StromConsensusEvent) {
        let blocknum = event.block_height();
        let _ = self
            .tx
            .send(TelemetryMessage::Consensus { blocknum, event });
    }

    fn consensus_state(&self, blocknum: u64, state: ConsensusRoundName) {
        let _ = self
            .tx
            .send(TelemetryMessage::ConsensusStateChange { blocknum, state });
    }

    fn gas_price_snapshot(
        &self,
        blocknum: u64,
        snapshot: (HashMap<PoolId, VecDeque<PairsWithPrice>>, u128)
    ) {
        let _ = self
            .tx
            .send(TelemetryMessage::GasPriceSnapshot { blocknum, snapshot });
    }

    fn error(&self, blocknum: u64, message: String) {
        let _ = self.tx.send(TelemetryMessage::Error { blocknum, message });
    }
}
