use std::{
    collections::{HashMap, VecDeque},
    pin::Pin,
    task::Poll
};

use alloy_primitives::Address;
use angstrom_eth::telemetry::EthUpdaterSnapshot;
use angstrom_types::{
    contract_bindings::angstrom::Angstrom::PoolKey,
    pair_with_price::PairsWithPrice,
    primitive::{
        ANGSTROM_ADDRESS, ANGSTROM_DEPLOYED_BLOCK, CHAIN_ID, GAS_TOKEN_ADDRESS,
        POOL_MANAGER_ADDRESS, PoolId
    },
    uni_structure::BaselinePoolState
};
use blocklog::BlockLog;
use chrono::Utc;
use futures::{FutureExt, StreamExt, stream::FuturesUnordered};
use order_pool::telemetry::OrderPoolSnapshot;
use outputs::TelemetryOutput;
use reth_tasks::shutdown::GracefulShutdown;
use serde::{Deserialize, Serialize};
use telemetry_recorder::{TELEMETRY_SENDER, TelemetryMessage};
use tokio::sync::mpsc::UnboundedReceiver;
use tracing::warn;
use validation::telemetry::ValidationSnapshot;

use crate::outputs::s3::S3Storage;

pub mod blocklog;
pub mod outputs;

// 5 block lookbehind, simple const for now
const MAX_BLOCKS: usize = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConstants {
    pub node_address:          Address,
    pub angstrom_address:      Address,
    pub pool_manager_address:  Address,
    pub angstrom_deploy_block: u64,
    pub gas_token_address:     Address,
    pub chain_id:              u64
}

impl NodeConstants {
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

pub struct Telemetry {
    rx:                  UnboundedReceiver<TelemetryMessage>,
    node_consts:         NodeConstants,
    block_cache:         HashMap<u64, BlockLog>,
    outputs:             Vec<Box<dyn TelemetryOutput + Send + 'static>>,
    pending_submissions: FuturesUnordered<Pin<Box<dyn Future<Output = ()> + Send + 'static>>>,
    // Allows us to keep shutdown from trigging for awhile to collect all data and send it off.
    guard:               GracefulShutdown
}

impl Telemetry {
    pub fn new(
        rx: UnboundedReceiver<TelemetryMessage>,
        node_address: Address,
        guard: GracefulShutdown,
        outputs: Vec<Box<dyn TelemetryOutput + Send + 'static>>
    ) -> Self {
        let node_consts = NodeConstants {
            node_address,
            angstrom_address: *ANGSTROM_ADDRESS.get().unwrap(),
            pool_manager_address: *POOL_MANAGER_ADDRESS.get().unwrap(),
            angstrom_deploy_block: *ANGSTROM_DEPLOYED_BLOCK.get().unwrap(),
            gas_token_address: *GAS_TOKEN_ADDRESS.get().unwrap(),
            chain_id: *CHAIN_ID.get().unwrap()
        };
        Self {
            rx,
            node_consts,
            block_cache: HashMap::new(),
            outputs,
            guard,
            pending_submissions: FuturesUnordered::new()
        }
    }

    fn get_block(&mut self, blocknum: u64) -> &mut BlockLog {
        if self.block_cache.contains_key(&blocknum) {
            self.block_cache.get_mut(&blocknum).unwrap()
        } else {
            // We're adding a new item, so trim down to size
            while self.block_cache.len() >= MAX_BLOCKS {
                let oldest_key = self.block_cache.keys().copied().min().unwrap();
                if let Some(mut block) = self.block_cache.remove(&oldest_key) {
                    // If we have a block that doesn't have any errors when we remove it. We know
                    // that it hasn't been pushed so we will push it here.
                    if !block.has_error() {
                        for out in self.outputs.iter() {
                            block.set_node_constants(self.node_consts.clone());
                            self.pending_submissions.push(out.output(block.clone()));
                        }
                    }
                }
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

    fn on_error(
        &mut self,
        blocknum: u64,
        timestamp: chrono::DateTime<Utc>,
        error: String,
        backtrace: String
    ) {
        if let Some(block) = self.block_cache.get_mut(&blocknum) {
            block.error(error, timestamp, backtrace);

            for out in self.outputs.iter() {
                block.set_node_constants(self.node_consts.clone());
                if block.everything_to_replay() {
                    self.pending_submissions.push(out.output(block.clone()));
                }
            }
        } else {
            warn!(blocknum, "got a error for a block that doesn't exist");
        }
    }
}

impl Future for Telemetry {
    type Output = ();

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>
    ) -> std::task::Poll<Self::Output> {
        while let Poll::Ready(req) = self.rx.poll_recv(cx) {
            match req {
                // As long as we're getting snapshots, process them
                Some(req) => match req {
                    TelemetryMessage::EthSnapshot { blocknum, eth_snapshot } => {
                        let decoded: EthUpdaterSnapshot =
                            serde_json::from_value(eth_snapshot).unwrap();

                        self.get_block(blocknum).set_eth(decoded);
                    }
                    TelemetryMessage::OrderPoolSnapshot { blocknum, orderpool_snapshot } => {
                        let decoded: OrderPoolSnapshot =
                            serde_json::from_value(orderpool_snapshot).unwrap();

                        self.get_block(blocknum).set_orderpool(decoded);
                    }
                    TelemetryMessage::ValidationSnapshot { blocknum, validation_snapshot } => {
                        let decoded: ValidationSnapshot =
                            serde_json::from_value(validation_snapshot).unwrap();

                        self.get_block(blocknum).set_validation(decoded);
                    }
                    TelemetryMessage::NewBlock { blocknum, pool_keys, pool_snapshots } => {
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
                    TelemetryMessage::Error { blocknum, timestamp, message, backtrace } => {
                        self.on_error(blocknum, timestamp, message, backtrace);
                    }
                },
                // End of receiver stream should end this task as well
                None => {
                    return Poll::Ready(());
                }
            }
        }

        while let Poll::Ready(Some(_)) = self.pending_submissions.poll_next_unpin(cx) {}

        // We want to be careful here as we want to ensure that all readings have been
        // sent before this
        if let Poll::Ready(guard) = self.guard.poll_unpin(cx) {
            let cache = self.block_cache.clone();

            let mut futures = FuturesUnordered::new();
            for (_, mut block) in cache {
                if !block.has_error() {
                    block.error("shutdown has occurred".to_string(), Utc::now(), "".to_string());

                    for out in self.outputs.iter() {
                        block.set_node_constants(self.node_consts.clone());
                        if block.everything_to_replay() {
                            futures.push(out.output(block.clone()));
                        }
                    }
                }
            }

            // move gaurd and ensure everything gets sent
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .worker_threads(2)
                    .build()
                    .unwrap();

                rt.block_on(async move {
                    while (futures.next().await).is_some() {}
                    drop(guard);
                });
            });

            return Poll::Ready(());
        }

        Poll::Pending
    }
}

pub async fn init_telemetry(node_address: Address, shutdown_handle: GracefulShutdown) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let _ = TELEMETRY_SENDER.set(tx);
    let handles =
        vec![Box::new(S3Storage::new().await.unwrap()) as Box<dyn TelemetryOutput + Send>];

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .build()
            .unwrap();

        rt.block_on(Telemetry::new(rx, node_address, shutdown_handle, handles))
    });
}
