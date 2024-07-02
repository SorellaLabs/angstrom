use std::task::{Context, Poll};

use alloy_provider::{Provider, RootProvider};
use alloy_pubsub::{PubSubFrontend, Subscription};
use alloy_rpc_types_eth::Block;
use angstrom_eth::{
    handle::{EthCommand, EthHandle},
    manager::EthEvent
};
use futures::{Future, FutureExt};
use reth_tasks::TaskSpawner;
use tokio::sync::mpsc::{Receiver, Sender, UnboundedSender};
use tokio_stream::wrappers::ReceiverStream;

pub struct AnvilEthDataCleanser {
    /// our command receiver
    commander:          ReceiverStream<EthCommand>,
    /// people listening to events
    event_listeners:    Vec<UnboundedSender<EthEvent>>,
    provider:           RootProvider<PubSubFrontend>,
    block_subscription: Subscription<Block>
}

impl AnvilEthDataCleanser {
    pub async fn spawn<TP: TaskSpawner>(
        tp: TP,
        tx: Sender<EthCommand>,
        rx: Receiver<EthCommand>,
        provider: RootProvider<PubSubFrontend>
    ) -> eyre::Result<EthHandle> {
        let stream = ReceiverStream::new(rx);
        let block_subscription = provider.subscribe_blocks().await?;
        let this =
            Self { commander: stream, event_listeners: Vec::new(), provider, block_subscription };
        tp.spawn_critical("eth handle", this.boxed());

        let handle = EthHandle::new(tx);

        Ok(handle)
    }

    fn send_events(&mut self, event: EthEvent) {
        self.event_listeners
            .retain(|e| e.send(event.clone()).is_ok());
    }

    fn on_command(&mut self, command: EthCommand) {
        match command {
            EthCommand::SubscribeEthNetworkEvents(tx) => self.event_listeners.push(tx)
        }
    }

    fn on_new_block(&mut self, block: Block) {
        let bn = block
            .header
            .number
            .expect("block from anvil with no number");

        self.send_events(EthEvent::NewBlock(bn));

        // let filled_orders =
    }
}

impl Future for AnvilEthDataCleanser {
    type Output = ();

    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Pending
    }
}
