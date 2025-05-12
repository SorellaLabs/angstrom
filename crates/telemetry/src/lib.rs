use std::{collections::HashMap, task::Poll};

use angstrom_types::{
    orders::{CancelOrderRequest, OrderOrigin},
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

#[derive(Serialize, Deserialize)]
pub enum TelemetryMessage {
    /// Message indicating that a new block has begun.  Sent by the pool manager
    /// with the updated pool snapshot for that block
    NewBlock { blocknum: u64, pool_snapshots: HashMap<PoolId, BaselinePoolState> },
    /// Message indicating an incoming order to be validated
    NewOrder { blocknum: u64, origin: OrderOrigin, order: AllOrders },
    /// Request to cancel an order
    CancelOrder { blocknum: u64, cancel: CancelOrderRequest },
    /// Message indicating an incoming Consensus message
    Consensus { blocknum: u64 },
    /// Message indicating an error has happened, marking a block for output
    Error { blocknum: u64, message: String }
}

pub struct Telemetry {
    rx:          UnboundedReceiver<TelemetryMessage>,
    block_cache: HashMap<u64, BlockLog>,
    outputs:     Vec<Box<dyn TelemetryOutput>>
}

impl Telemetry {
    pub fn new(rx: UnboundedReceiver<TelemetryMessage>) -> Self {
        // By default let's just turn on all our outputs, we only have one
        let outputs: Vec<Box<dyn TelemetryOutput>> = vec![Box::new(LogOutput {})];
        let block_cache = HashMap::new();
        Self { rx, block_cache, outputs }
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

    fn on_new_block(&mut self, blocknum: u64, pool_snapshots: HashMap<PoolId, BaselinePoolState>) {
        self.get_block(blocknum).set_pool_snapshots(pool_snapshots);
    }

    fn add_event_to_block(&mut self, blocknum: u64, event: TelemetryMessage) {
        self.get_block(blocknum).add_event(event);
    }

    fn on_error(&mut self, blocknum: u64, error: String) {
        if let Some(block) = self.block_cache.get_mut(&blocknum) {
            block.error(error);
            for out in self.outputs.iter() {
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
                    TelemetryMessage::NewBlock { blocknum, pool_snapshots } => {
                        println!("New block [{blocknum}]");
                        self.on_new_block(blocknum, pool_snapshots);
                    }
                    event @ TelemetryMessage::NewOrder { blocknum, .. }
                    | event @ TelemetryMessage::CancelOrder { blocknum, .. } => {
                        self.add_event_to_block(blocknum, event);
                    }
                    _ => println!("Unhandled!")
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

pub fn init_telemetry() -> TelemetryClient {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(4)
            .build()
            .unwrap();

        rt.block_on(Telemetry::new(rx))
    });
    TelemetryClient::new(tx)
}
