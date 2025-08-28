use std::pin::Pin;

use futures::Future;
use futures_util::Stream;
use reth_primitives::{EthPrimitives, NodePrimitives};
use reth_provider::CanonStateNotification;
use tokio::sync::mpsc::{Sender, UnboundedSender, unbounded_channel};
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::manager::EthEvent;

pub trait Eth<N: NodePrimitives>: Clone + Send + Sync {
    fn subscribe_network_stream(&self) -> Pin<Box<dyn Stream<Item = EthEvent> + Send>> {
        Box::pin(self.subscribe_network())
    }

    fn subscribe_network(&self) -> UnboundedReceiverStream<EthEvent>;
    fn subscribe_cannon_state_notifications(
        &self
    ) -> impl Future<Output = tokio::sync::broadcast::Receiver<CanonStateNotification<N>>> + Send;
}

pub enum EthCommand<N: NodePrimitives = EthPrimitives> {
    SubscribeEthNetworkEvents(UnboundedSender<EthEvent>),
    SubscribeCannon(
        tokio::sync::oneshot::Sender<tokio::sync::broadcast::Receiver<CanonStateNotification<N>>>
    )
}

#[derive(Debug, Clone)]
pub struct EthHandle<N: NodePrimitives = EthPrimitives> {
    pub sender: Sender<EthCommand<N>>
}

impl<N: NodePrimitives> EthHandle<N> {
    pub fn new(sender: Sender<EthCommand<N>>) -> Self {
        Self { sender }
    }
}

impl<N: NodePrimitives> Eth<N> for EthHandle<N> {
    async fn subscribe_cannon_state_notifications(
        &self
    ) -> tokio::sync::broadcast::Receiver<CanonStateNotification<N>> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = self.sender.send(EthCommand::<N>::SubscribeCannon(tx)).await;
        rx.await.unwrap()
    }

    fn subscribe_network(&self) -> UnboundedReceiverStream<EthEvent> {
        let (tx, rx) = unbounded_channel();
        let _ = self
            .sender
            .try_send(EthCommand::<N>::SubscribeEthNetworkEvents(tx));

        UnboundedReceiverStream::new(rx)
    }
}
