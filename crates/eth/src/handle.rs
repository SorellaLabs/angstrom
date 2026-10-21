use std::pin::Pin;

use angstrom_types::contract_payloads::protocol_fees::DonationSplitSnapshot;
use futures::Future;
use futures_util::Stream;
use reth_provider::CanonStateNotification;
use tokio::sync::mpsc::{Sender, UnboundedSender, unbounded_channel};
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::manager::EthEvent;

pub trait Eth: Clone + Send + Sync {
    fn subscribe_network_stream(&self) -> Pin<Box<dyn Stream<Item = EthEvent> + Send>> {
        Box::pin(self.subscribe_network())
    }

    fn subscribe_network(&self) -> UnboundedReceiverStream<EthEvent>;

    /// The event stream plus the protocol fee config in force when the stream
    /// was opened. Read where the listener is registered, so a publication can
    /// neither be missed between the two nor arrive twice with different
    /// values.
    fn subscribe_network_with_config(
        &self
    ) -> impl Future<Output = (UnboundedReceiverStream<EthEvent>, DonationSplitSnapshot)> + Send;

    fn subscribe_cannon_state_notifications(
        &self
    ) -> impl Future<Output = tokio::sync::broadcast::Receiver<CanonStateNotification>> + Send;

    /// Lets the cleanser start applying canonical updates. Until this is
    /// called it holds the backlog unread, so no block is applied and no
    /// block-sync proposal is opened before every module has registered and
    /// subscribed. Call it last in startup, after `finalize_modules()`.
    fn release_canonical_updates(&self) -> impl Future<Output = ()> + Send;
}

pub enum EthCommand {
    SubscribeEthNetworkEvents(UnboundedSender<EthEvent>),
    SubscribeEthNetworkEventsWithConfig(
        UnboundedSender<EthEvent>,
        tokio::sync::oneshot::Sender<DonationSplitSnapshot>
    ),
    SubscribeCannon(
        tokio::sync::oneshot::Sender<tokio::sync::broadcast::Receiver<CanonStateNotification>>
    ),
    ReleaseCanonicalUpdates(tokio::sync::oneshot::Sender<()>)
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

    /// Sent with the awaiting `send`, so it queues strictly behind every
    /// earlier subscribe command rather than racing them.
    async fn release_canonical_updates(&self) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = self
            .sender
            .send(EthCommand::ReleaseCanonicalUpdates(tx))
            .await;
        // a cleanser that has already exited must not panic startup
        let _ = rx.await;
    }

    async fn subscribe_network_with_config(
        &self
    ) -> (UnboundedReceiverStream<EthEvent>, DonationSplitSnapshot) {
        let (tx, rx) = unbounded_channel();
        let (config_tx, config_rx) = tokio::sync::oneshot::channel();
        let _ = self
            .sender
            .send(EthCommand::SubscribeEthNetworkEventsWithConfig(tx, config_tx))
            .await;

        (UnboundedReceiverStream::new(rx), config_rx.await.unwrap())
    }
}
