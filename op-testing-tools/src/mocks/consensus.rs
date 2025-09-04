use std::time::Duration;

use alloy::primitives::Address;
use angstrom_types::consensus::{PreProposal, Proposal, StromConsensusEvent};
use tokio::{
    sync::{
        Mutex,
        mpsc::{UnboundedSender, unbounded_channel}
    },
    time::Interval
};
use tokio_stream::wrappers::UnboundedReceiverStream;

pub struct MockConsensusEventHandle {
    tx:       UnboundedSender<StromConsensusEvent>,
    interval: Mutex<Interval>
}

impl MockConsensusEventHandle {
    pub fn new() -> (Self, UnboundedReceiverStream<StromConsensusEvent>) {
        let (tx, rx) = unbounded_channel();

        // 15 second interval
        let mut interval = tokio::time::interval(Duration::from_secs(15));
        // If we miss, we want to make sure we're aligned with our original interval and
        // skip extra ticks
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        (Self { tx, interval: Mutex::new(interval) }, UnboundedReceiverStream::new(rx))
    }

    pub fn prepropose(&self, peer: Address, proposal: PreProposal) {
        self.tx
            .send(StromConsensusEvent::PreProposal(peer, proposal))
            .expect("Failed to send proposal");
    }

    pub fn propose(&self, peer: Address, proposal: Proposal) {
        self.tx
            .send(StromConsensusEvent::Proposal(peer, proposal))
            .expect("Failed to send proposal");
    }

    pub async fn propose_on_next_tick(&self, peer: Address, proposal: Proposal) {
        let mut i = self.interval.lock().await;
        i.tick().await;
        self.propose(peer, proposal);
    }
}
