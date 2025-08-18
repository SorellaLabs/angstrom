use angstrom_cli::handles::{ConsensusHandles, DefaultPoolHandle};
use angstrom_eth::handle::EthCommand;
use angstrom_network::NetworkOrderEvent;
use angstrom_types::consensus::StromConsensusEvent;
use order_pool::PoolManagerUpdate;
use pool_manager::{OrderCommand, PoolHandle};
use reth_metrics::common::mpsc::UnboundedMeteredSender;
use tokio::sync::mpsc::{Sender, UnboundedSender};

#[derive(Clone)]
pub struct SendingStromHandles {
    pub eth_tx:          Sender<EthCommand>,
    pub network_tx:      UnboundedMeteredSender<NetworkOrderEvent>,
    pub orderpool_tx:    UnboundedSender<OrderCommand>,
    pub pool_manager_tx: tokio::sync::broadcast::Sender<PoolManagerUpdate>,
    // pub consensus_tx:    Sender<ConsensusMessage>,
    pub consensus_tx_op: UnboundedMeteredSender<StromConsensusEvent>
}

impl SendingStromHandles {
    pub fn get_pool_handle(&self) -> DefaultPoolHandle {
        PoolHandle {
            manager_tx:      self.orderpool_tx.clone(),
            pool_manager_tx: self.pool_manager_tx.clone()
        }
    }
}

impl From<&ConsensusHandles> for SendingStromHandles {
    fn from(value: &ConsensusHandles) -> Self {
        Self {
            eth_tx:          value.eth_tx.clone(),
            network_tx:      value.pool_tx.clone(),
            orderpool_tx:    value.orderpool_tx.clone(),
            pool_manager_tx: value.pool_manager_tx.clone(),
            // consensus_tx:    value.consensus_tx.clone(),
            consensus_tx_op: value.mode.consensus_tx_op.clone()
        }
    }
}
