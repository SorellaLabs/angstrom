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

    pub fn block_state_transition(
        &self,
        block_number: u64,
        filled_orders: Vec<B256>,
        address_changeset: Vec<Address>
    ) {
        self.tx
            .send(EthEvent::NewBlockTransitions { block_number, filled_orders, address_changeset })
            .expect("failed to send");
    }

    pub fn finalize_block(&self, block: u64) {
        self.tx
            .send(EthEvent::FinalizedBlock(block))
            .expect("state changes")
    }

    pub fn reorged_orders(&self, orders: Vec<B256>) {
        self.tx
            .send(EthEvent::ReorgedOrders(orders, 0..=0))
            .expect("state changes")
    }
}

#[derive(Default, Clone)]
pub struct MockEthSubscription {
    subscribers: Vec<UnboundedSender<EthEvent>>
}

impl MockEthSubscription {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn subscribe(&mut self) -> UnboundedReceiverStream<EthEvent> {
        let (tx, rx) = unbounded_channel();
        self.subscribers.push(tx);
        UnboundedReceiverStream::new(rx)
    }

    pub fn trigger_new_block(&self, block: u64) {
        for s in self.subscribers.iter() {
            s.send(EthEvent::NewBlock(block)).expect("failed to send");
        }
    }
}
