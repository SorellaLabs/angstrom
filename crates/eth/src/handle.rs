use std::pin::Pin;

use futures_util::Stream;
use tokio::sync::mpsc::{unbounded_channel, Sender, UnboundedSender};
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::manager::EthEvent;

pub trait Eth: Clone + Send + Sync {
    fn subscribe_network_stream(&self) -> Pin<Box<dyn Stream<Item = EthEvent> + Send>> {
        Box::pin(self.subscribe_network())
    }

    fn subscribe_network(&self) -> UnboundedReceiverStream<EthEvent>;
}

pub enum EthCommand {
    SubscribeEthNetworkEvents(UnboundedSender<EthEvent>)
}

#[derive(Debug, Clone)]
pub struct EthHandle {
    pub sender: Sender<EthCommand>
}

impl EthHandle {
    pub fn new(sender: Sender<EthCommand>) -> Self {
        Self { sender }
    }
}

impl Eth for EthHandle {
    fn subscribe_network(&self) -> UnboundedReceiverStream<EthEvent> {
        let (tx, rx) = unbounded_channel();
        let _ = self
            .sender
            .try_send(EthCommand::SubscribeEthNetworkEvents(tx));

        UnboundedReceiverStream::new(rx)
    }
}
