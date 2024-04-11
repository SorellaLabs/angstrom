use alloy_primitives::{Address, B256};
use angstrom_eth::manager::EthEvent;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio_stream::wrappers::UnboundedReceiverStream;

#[derive(Debug, Clone)]
pub struct MockEthEventHandle {
    pub tx: UnboundedSender<EthEvent>
}

impl MockEthEventHandle {
    pub fn new() -> (Self, UnboundedReceiverStream<EthEvent>) {
        let (tx, rx) = unbounded_channel();
        (Self { tx }, UnboundedReceiverStream::new(rx))
    }

    pub fn trigger_new_block(&self, block: u64) {
        self.tx
            .send(EthEvent::NewBlock(block))
            .expect("failed to send");
    }

    pub fn filled_orders(&self, block: u64, hashes: Vec<B256>) {
        self.tx
            .send(EthEvent::FilledOrders(hashes, block))
            .expect("failed to send");
    }

    pub fn state_changes(&self, addresses: Vec<Address>) {
        self.tx
            .send(EthEvent::EOAStateChanges(addresses))
            .expect("state changes")
    }

    pub fn finalize_block(&self, block: u64) {
        self.tx
            .send(EthEvent::FinalizedBlock(block))
            .expect("state changes")
    }

    pub fn reorged_orders(&self, orders: Vec<B256>) {
        self.tx
            .send(EthEvent::ReorgedOrders(orders))
            .expect("state changes")
    }
}
