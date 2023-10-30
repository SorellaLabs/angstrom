use std::sync::Arc;

use ethers_core::types::{Block, H256};
use ethers_flashbots::PendingBundleError;
use futures::Stream;
use futures_util::StreamExt;
use tokio::sync::mpsc::*;
use tokio_stream::wrappers::ReceiverStream;

use crate::{manager::EthNetworkEvent, EthSubscribe};

pub struct EthHandle {
    sender: Sender<EthCommand>
}

impl EthHandle {
    pub fn new(sender: Sender<EthCommand>) -> Self {
        Self { sender }
    }
}

#[async_trait::async_trait]
impl EthSubscribe for EthHandle {
    async fn subscribe_new_blocks(&self) -> Box<dyn Stream<Item = Arc<Block<H256>>> + Send> {
        let (tx, rx) = channel(5);
        let rec: ReceiverStream<EthNetworkEvent> = ReceiverStream::new(rx);
        let _ = self.sender.try_send(EthCommand::Subscribe(tx));

        Box::new(rec.filter_map(|event| async move { event.try_new_block() }))
    }

    async fn fetch_to_tip(
        &self,
        start_block: u64
    ) -> Box<dyn Stream<Item = Arc<Block<H256>>> + Send> {
        let (tx, rx) = channel(5);
        let rec: ReceiverStream<EthNetworkEvent> = ReceiverStream::new(rx);
        let _ = self
            .sender
            .try_send(EthCommand::SubscribeSync { start_block, sender: tx });

        Box::new(rec.filter_map(|event| async move { event.try_sync_block() }))
    }

    async fn subscribe_leader_submission(&self) -> ReceiverStream<Result<(), PendingBundleError>> {
        todo!()
    }
}

pub enum EthCommand {
    Subscribe(Sender<EthNetworkEvent>),
    SubscribeSync { start_block: u64, sender: Sender<EthNetworkEvent> }
}
