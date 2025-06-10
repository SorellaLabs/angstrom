use std::{
    collections::{HashMap, VecDeque},
    task::Poll
};

use alloy_primitives::Address;
use angstrom_types::{
    consensus::{ConsensusRoundName, StromConsensusEvent},
    contract_bindings::angstrom::Angstrom::PoolKey,
    orders::{CancelOrderRequest, OrderOrigin},
    pair_with_price::PairsWithPrice,
    primitive::PoolId,
    sol_bindings::grouped_orders::AllOrders,
    uni_structure::BaselinePoolState
};
use blocklog::BlockLog;
use client::TelemetryClient;
use outputs::{TelemetryOutput, log::LogOutput};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedReceiver;
use tracing::warn;

pub mod blocklog;
pub mod client;
pub mod outputs;
pub mod replay;

// 5 block lookbehind, simple const for now
const MAX_BLOCKS: usize = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConstants {
    node_address:          Address,
    angstrom_address:      Address,
    pool_manager_address:  Address,
    angstrom_deploy_block: u64,
    gas_token_address:     Address
}

impl NodeConstants {
    pub fn new(
        node_address: Address,
        angstrom_address: Address,
        pool_manager_address: Address,
        angstrom_deploy_block: u64,
        gas_token_address: Address
    ) -> Self {
        Self {
            node_address,
            angstrom_address,
            pool_manager_address,
            angstrom_deploy_block,
            gas_token_address
        }
    }

    pub fn node_address(&self) -> Address {
        self.node_address
    }

    pub fn angstrom_address(&self) -> Address {
        self.angstrom_address
    }

    pub fn pool_manager_address(&self) -> Address {
        self.pool_manager_address
    }

    pub fn gas_token_address(&self) -> Address {
        self.gas_token_address
    }

    pub fn angstrom_deploy_block(&self) -> u64 {
        self.angstrom_deploy_block
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TelemetryMessage {
    /// Message indicating that a new block has begun.  Sent by the pool manager
    /// with the updated pool snapshot for that block
    NewBlock {
        blocknum:       u64,
        pool_keys:      Vec<PoolKey>,
        pool_snapshots: HashMap<PoolId, BaselinePoolState>
    },
    /// Message indicating an incoming order to be validated
    NewOrder {
        blocknum: u64,
        origin:   OrderOrigin,
        order:    AllOrders
    },
    /// Request to cancel an order
    CancelOrder {
        blocknum: u64,
        cancel:   CancelOrderRequest
    },
    /// Message indicating an incoming Consensus message
    Consensus {
        blocknum: u64,
        event:    StromConsensusEvent
    },
    ConsensusStateChange {
        blocknum: u64,
        state:    ConsensusRoundName
    },
    /// Message assigning a snapshot of our gas prices to this block
    GasPriceSnapshot {
        blocknum: u64,
        snapshot: (HashMap<PoolId, VecDeque<PairsWithPrice>>, u128)
    },
    /// Message indicating an error has happened, marking a block for output
    Error {
        blocknum: u64,
        message:  String
    }
}

pub struct Telemetry {
    rx:          UnboundedReceiver<TelemetryMessage>,
    node_consts: NodeConstants,
    block_cache: HashMap<u64, BlockLog>,
    outputs:     Vec<Box<dyn TelemetryOutput>>
}

impl Telemetry {
    pub fn new(rx: UnboundedReceiver<TelemetryMessage>, node_consts: NodeConstants) -> Self {
        // By default let's just turn on all our outputs, we only have one
        let outputs: Vec<Box<dyn TelemetryOutput>> = vec![Box::new(LogOutput {})];
        let block_cache = HashMap::new();
        Self { rx, node_consts, block_cache, outputs }
    }

    fn get_block(&mut self, blocknum: u64) -> &mut BlockLog {
        if self.block_cache.contains_key(&blocknum) {
            self.block_cache.get_mut(&blocknum).unwrap()
        } else {
            // We're adding a new item, so trim down to size
            while self.block_cache.len() >= MAX_BLOCKS {
                let oldest_key = self.block_cache.keys().copied().min().unwrap();
                self.block_cache.remove(&oldest_key);
            }
            // Add our new entry
            self.block_cache
                .entry(blocknum)
                .or_insert_with(|| BlockLog::new(blocknum))
        }
    }

    fn on_new_block(
        &mut self,
        blocknum: u64,
        pool_keys: Vec<PoolKey>,
        pool_snapshots: HashMap<PoolId, BaselinePoolState>
    ) {
        let block = self.get_block(blocknum);
        block.set_pool_snapshots(pool_snapshots);
        block.set_pool_keys(pool_keys);
    }

    fn add_event_to_block(&mut self, blocknum: u64, event: TelemetryMessage) {
        self.get_block(blocknum).add_event(event);
    }

    fn add_gas_snapshot_to_block(
        &mut self,
        blocknum: u64,
        snapshot: (HashMap<PoolId, VecDeque<PairsWithPrice>>, u128)
    ) {
        self.get_block(blocknum).set_gas_price_snapshot(snapshot);
    }

    fn on_error(&mut self, blocknum: u64, error: String) {
        if let Some(block) = self.block_cache.get_mut(&blocknum) {
            block.error(error);
            for out in self.outputs.iter() {
                block.set_node_constants(self.node_consts.clone());
                out.output(block)
            }
        } else {
            warn!(blocknum, "Telemetry error for unrecorded block");
        }
    }
}

impl Future for Telemetry {
    type Output = ();

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>
    ) -> std::task::Poll<Self::Output> {
        loop {
            match self.rx.poll_recv(cx) {
                // As long as we're getting snapshots, process them
                Poll::Ready(Some(req)) => match req {
                    TelemetryMessage::NewBlock { blocknum, pool_keys, pool_snapshots } => {
                        println!("New block [{blocknum}]");
                        self.on_new_block(blocknum, pool_keys, pool_snapshots);
                    }
                    event @ TelemetryMessage::NewOrder { blocknum, .. }
                    | event @ TelemetryMessage::CancelOrder { blocknum, .. }
                    | event @ TelemetryMessage::ConsensusStateChange { blocknum, .. }
                    | event @ TelemetryMessage::Consensus { blocknum, .. } => {
                        self.add_event_to_block(blocknum, event);
                    }
                    TelemetryMessage::GasPriceSnapshot { blocknum, snapshot } => {
                        self.add_gas_snapshot_to_block(blocknum, snapshot);
                    }
                    TelemetryMessage::Error { blocknum, message } => {
                        self.on_error(blocknum, message);
                    }
                },
                // End of receiver stream should end this task as well
                Poll::Ready(None) => {
                    return Poll::Ready(());
                }
                // Otherwise we're scheduled to wake on next message, so let's pend
                Poll::Pending => {
                    return Poll::Pending;
                }
            }
        }
    }
}

pub fn init_telemetry(node_consts: NodeConstants) -> TelemetryClient {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(4)
            .build()
            .unwrap();

        rt.block_on(Telemetry::new(rx, node_consts))
    });
    TelemetryClient::new(tx)
}
