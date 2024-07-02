use std::task::{Context, Poll};

use alloy_primitives::{hex, Address};
use alloy_provider::{Provider, RootProvider};
use alloy_pubsub::{PubSubFrontend, Subscription};
use alloy_rpc_types_eth::Block;
use angstrom_eth::{
    handle::{EthCommand, EthHandle},
    manager::EthEvent
};
use futures::{Future, FutureExt, Stream, StreamExt};
use reth_tasks::TaskSpawner;
use tokio::sync::mpsc::{Receiver, Sender, UnboundedSender};
use tokio_stream::wrappers::ReceiverStream;

pub struct AnvilEthDataCleanser<S: Stream<Item = Block>> {
    angstrom_contract:  Address,
    /// our command receiver
    commander:          ReceiverStream<EthCommand>,
    /// people listening to events
    event_listeners:    Vec<UnboundedSender<EthEvent>>,
    provider:           RootProvider<PubSubFrontend>,
    block_subscription: S
}

impl<S: Stream<Item = Block> + Unpin + Send + 'static> AnvilEthDataCleanser<S> {
    pub async fn spawn<TP: TaskSpawner>(
        tp: TP,
        angstrom_contract: Address,
        tx: Sender<EthCommand>,
        rx: Receiver<EthCommand>,
        provider: RootProvider<PubSubFrontend>,
        block_subscription: S
    ) -> eyre::Result<EthHandle> {
        let stream = ReceiverStream::new(rx);
        let this = Self {
            commander: stream,
            event_listeners: Vec::new(),
            provider,
            block_subscription,
            angstrom_contract
        };
        tp.spawn_critical("eth handle", Box::pin(this));

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
        // find angstrom tx
        let Some(_angstrom_tx) = block
            .transactions
            .txns()
            .find(|tx| tx.to == Some(self.angstrom_contract))
        else {
            tracing::info!("No angstrom tx found");
            return
        };
    }
}

impl<S: Stream<Item = Block> + Unpin + Send + 'static> Future for AnvilEthDataCleanser<S> {
    type Output = ();

    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        while let Poll::Ready(Some(block)) = self.block_subscription.poll_next_unpin(cx) {
            self.on_new_block(block);
        }
        while let Poll::Ready(Some(cmd)) = self.commander.poll_next_unpin(cx) {
            self.on_command(cmd);
        }

        Poll::Pending
    }
}
