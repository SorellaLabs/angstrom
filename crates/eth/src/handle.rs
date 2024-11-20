use std::pin::Pin;

use futures::Future;
use futures_util::Stream;
use reth_provider::CanonStateNotification;
use tokio::sync::mpsc::{unbounded_channel, Sender, UnboundedSender};
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::manager::EthEvent;

pub trait Eth: Clone + Send + Sync {
    fn subscribe_network_stream(&self) -> Pin<Box<dyn Stream<Item = EthEvent> + Send>> {
        Box::pin(self.subscribe_network())
    }

    fn subscribe_network(&self) -> UnboundedReceiverStream<EthEvent>;
    fn subscribe_cannon_state_notifications(
        &self
    ) -> impl Future<Output = tokio::sync::broadcast::Receiver<CanonStateNotification>> + Send;
}

pub enum EthCommand {
    SubscribeEthNetworkEvents(UnboundedSender<EthEvent>),
    SubscribeCannon(
        tokio::sync::oneshot::Sender<tokio::sync::broadcast::Receiver<CanonStateNotification>>
    )
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
    async fn subscribe_cannon_state_notifications(
        &self
    ) -> tokio::sync::broadcast::Receiver<CanonStateNotification> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = self.sender.send(EthCommand::SubscribeCannon(tx)).await;
        rx.await.unwrap()
    }

    fn subscribe_network(&self) -> UnboundedReceiverStream<EthEvent> {
        let (tx, rx) = unbounded_channel();
        let _ = self
            .sender
            .try_send(EthCommand::SubscribeEthNetworkEvents(tx));

        UnboundedReceiverStream::new(rx)
    }
}
