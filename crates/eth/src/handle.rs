use ethers_core::types::{Block, H256};
use ethers_flashbots::PendingBundleError;
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
    async fn subscribe_new_blocks(&self) -> ReceiverStream<Block<H256>> {
        let (tx, rx) = channel(5);
        let rec: ReceiverStream<EthNetworkEvent> = ReceiverStream::new(rx);
        let _ = self.sender.try_send(EthCommand::Subscribe(tx));

        todo!()
    }

    async fn subscribe_sync(&self, start_block: u64) -> ReceiverStream<Block<H256>> {
        todo!()
    }

    async fn subscribe_leader_submission(&self) -> ReceiverStream<Result<(), PendingBundleError>> {
        todo!()
    }
}

pub enum EthCommand {
    Subscribe(Sender<EthNetworkEvent>),
    SubscribeSync { start_block: u64, sender: Sender<EthNetworkEvent> }
}
