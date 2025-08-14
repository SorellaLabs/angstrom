use std::collections::HashSet;

use angstrom_amm_quoter::Slot0Update;
use angstrom_eth::{handle::EthCommand, manager::EthEvent};
use angstrom_network::{
    NetworkOrderEvent,
    pool_manager::{OrderCommand, PoolHandle}
};
use angstrom_types::{consensus::StromConsensusEvent, primitive::PoolId};
use matching_engine::manager::MatcherCommand;
use order_pool::PoolManagerUpdate;
use reth::primitives::EthPrimitives;
use reth_metrics::common::mpsc::{UnboundedMeteredReceiver, UnboundedMeteredSender};
use reth_node_builder::NodePrimitives;
use reth_optimism_primitives::OpPrimitives;
use serde::Serialize;
use tokio::sync::{
    mpsc,
    mpsc::{Receiver, Sender, UnboundedReceiver, UnboundedSender, channel, unbounded_channel}
};
use validation::validator::ValidationRequest;

pub type DefaultPoolHandle = PoolHandle;
type DefaultOrderCommand = OrderCommand;

pub(crate) trait AngstromMode {
    type Primitives: NodePrimitives + Serialize;
}

pub(crate) struct ConsensusMode {
    pub consensus_tx_op: UnboundedMeteredSender<StromConsensusEvent>,
    pub consensus_rx_op: UnboundedMeteredReceiver<StromConsensusEvent>,

    pub consensus_tx_rpc: UnboundedSender<consensus::ConsensusRequest>,
    pub consensus_rx_rpc: UnboundedReceiver<consensus::ConsensusRequest>
}

pub(crate) struct RollupMode;

impl AngstromMode for ConsensusMode {
    type Primitives = EthPrimitives;
}

impl AngstromMode for RollupMode {
    type Primitives = OpPrimitives;
}

// due to how the init process works with reth. we need to init like this
pub(crate) struct StromHandles<M: AngstromMode> {
    pub eth_tx: Sender<EthCommand<M::Primitives>>,
    pub eth_rx: Receiver<EthCommand<M::Primitives>>,

    pub pool_tx: UnboundedMeteredSender<NetworkOrderEvent>,
    pub pool_rx: UnboundedMeteredReceiver<NetworkOrderEvent>,

    pub orderpool_tx: UnboundedSender<DefaultOrderCommand>,
    pub orderpool_rx: UnboundedReceiver<DefaultOrderCommand>,

    pub quoter_tx: mpsc::Sender<(HashSet<PoolId>, mpsc::Sender<Slot0Update>)>,
    pub quoter_rx: mpsc::Receiver<(HashSet<PoolId>, mpsc::Sender<Slot0Update>)>,

    pub validator_tx: UnboundedSender<ValidationRequest>,
    pub validator_rx: UnboundedReceiver<ValidationRequest>,

    pub eth_handle_tx: Option<UnboundedSender<EthEvent>>,
    pub eth_handle_rx: Option<UnboundedReceiver<EthEvent>>,

    pub pool_manager_tx: tokio::sync::broadcast::Sender<PoolManagerUpdate>,

    // only 1 set cur
    pub matching_tx: Sender<MatcherCommand>,
    pub matching_rx: Receiver<MatcherCommand>,

    pub mode: M
}

impl<M: AngstromMode> StromHandles<M> {
    pub fn get_pool_handle(&self) -> DefaultPoolHandle {
        PoolHandle {
            manager_tx:      self.orderpool_tx.clone(),
            pool_manager_tx: self.pool_manager_tx.clone()
        }
    }
}

/// [`StromHandles`] for consensus mode.
pub(crate) type ConsensusHandles = StromHandles<ConsensusMode>;

impl ConsensusHandles {
    /// Creates a new set of handles for consensus mode.
    pub fn new() -> ConsensusHandles {
        let (eth_tx, eth_rx) = channel(100);
        let (matching_tx, matching_rx) = channel(100);
        let (pool_manager_tx, _) = tokio::sync::broadcast::channel(100);
        let (pool_tx, pool_rx) = reth_metrics::common::mpsc::metered_unbounded_channel("orderpool");
        let (orderpool_tx, orderpool_rx) = unbounded_channel();
        let (validator_tx, validator_rx) = unbounded_channel();
        let (eth_handle_tx, eth_handle_rx) = unbounded_channel();
        let (quoter_tx, quoter_rx) = channel(1000);
        let (consensus_tx_rpc, consensus_rx_rpc) = unbounded_channel();
        let (consensus_tx_op, consensus_rx_op) =
            reth_metrics::common::mpsc::metered_unbounded_channel("orderpool");

        let consensus_mode =
            ConsensusMode { consensus_tx_op, consensus_rx_op, consensus_tx_rpc, consensus_rx_rpc };

        StromHandles {
            eth_tx,
            quoter_tx,
            quoter_rx,
            eth_rx,
            pool_tx,
            pool_rx,
            orderpool_tx,
            orderpool_rx,
            validator_tx,
            validator_rx,
            pool_manager_tx,
            matching_tx,
            matching_rx,
            eth_handle_tx: Some(eth_handle_tx),
            eth_handle_rx: Some(eth_handle_rx),
            mode: consensus_mode
        }
    }
}

/// [`StromHandles`] for rollup mode.
pub(crate) type RollupHandles = StromHandles<RollupMode>;

impl RollupHandles {
    /// Creates a new set of handles for rollup mode.
    pub fn new() -> RollupHandles {
        let (eth_tx, eth_rx) = channel(100);
        let (matching_tx, matching_rx) = channel(100);
        let (pool_manager_tx, _) = tokio::sync::broadcast::channel(100);
        let (pool_tx, pool_rx) = reth_metrics::common::mpsc::metered_unbounded_channel("orderpool");
        let (orderpool_tx, orderpool_rx) = unbounded_channel();
        let (validator_tx, validator_rx) = unbounded_channel();
        let (eth_handle_tx, eth_handle_rx) = unbounded_channel();
        let (quoter_tx, quoter_rx) = channel(1000);

        StromHandles {
            eth_tx,
            quoter_tx,
            quoter_rx,
            eth_rx,
            pool_tx,
            pool_rx,
            orderpool_tx,
            orderpool_rx,
            validator_tx,
            validator_rx,
            pool_manager_tx,
            matching_tx,
            matching_rx,
            eth_handle_tx: Some(eth_handle_tx),
            eth_handle_rx: Some(eth_handle_rx),
            mode: RollupMode
        }
    }
}
