use std::sync::{Arc, atomic::AtomicUsize};

use angstrom_eth::manager::EthEvent;
use angstrom_network::{NetworkOrderEvent, StromNetworkHandle, StromNetworkHandleMsg};
use angstrom_types::consensus::StromConsensusEvent;
use reth_metrics::common::mpsc::UnboundedMeteredSender;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_stream::wrappers::UnboundedReceiverStream;

pub struct FakeNetwork {
    pub to_pool_manager:      Option<UnboundedMeteredSender<NetworkOrderEvent>>,
    pub to_consensus_manager: Option<UnboundedMeteredSender<StromConsensusEvent>>,
    pub eth_handle:           UnboundedReceiver<EthEvent>,
    pub from_handle_rx:       UnboundedReceiverStream<StromNetworkHandleMsg>,
    pub handle:               StromNetworkHandle
}

impl FakeNetwork {
    pub fn new(
        to_pool_manager: Option<UnboundedMeteredSender<NetworkOrderEvent>>,
        to_consensus_manager: Option<UnboundedMeteredSender<StromConsensusEvent>>,
        eth_handle: UnboundedReceiver<EthEvent>
    ) -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let peers = Arc::new(AtomicUsize::default());
        let handle =
            StromNetworkHandle::new(peers.clone(), UnboundedMeteredSender::new(tx, "strom handle"));

        Self {
            handle,
            to_pool_manager,
            to_consensus_manager,
            eth_handle,
            from_handle_rx: UnboundedReceiverStream::new(rx)
        }
    }
}
