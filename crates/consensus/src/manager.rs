use std::{
    pin::Pin,
    task::{Context, Poll}
};

use angstrom_network::{manager::StromConsensusEvent, StromNetworkHandle};
use angstrom_utils::PollExt;
use futures::{Future, FutureExt, Stream, StreamExt};
use order_pool::OrderPoolHandle;
use reth_metrics::common::mpsc::UnboundedMeteredReceiver;
use reth_provider::CanonStateNotifications;
use reth_tasks::TaskSpawner;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio_stream::wrappers::ReceiverStream;
use tracing::warn;
use validation::bundle::BundleValidator;

use crate::{
    core::{ConsensusCore, ConsensusMessage},
    ConsensusListener, ConsensusUpdater
};

#[allow(unused)]
pub struct ConsensusManager<OrderPool, Validator> {
    core: ConsensusCore,

    command:                ReceiverStream<ConsensusCommand>,
    subscribers:            Vec<Sender<ConsensusMessage>>,
    /// Used to trigger new consensus rounds
    canonical_block_stream: CanonStateNotifications,
    /// events from the network,
    strom_consensus_event:  UnboundedMeteredReceiver<StromConsensusEvent>,

    network: StromNetworkHandle,

    orderpool: OrderPool,
    validator: Validator
}

impl<OrderPool, Validator> ConsensusManager<OrderPool, Validator>
where
    OrderPool: OrderPoolHandle,
    Validator: BundleValidator
{
    pub fn new<TP: TaskSpawner>(
        tp: TP,
        network: StromNetworkHandle,
        orderpool: OrderPool,
        validator: Validator,
        canonical_block_stream: CanonStateNotifications,
        strom_consensus_event: UnboundedMeteredReceiver<StromConsensusEvent>,
        tx: Sender<ConsensusCommand>,
        rx: Receiver<ConsensusCommand>
    ) -> ConsensusHandle {
        let stream = ReceiverStream::new(rx);
        let (core, _bn) = ConsensusCore::new();

        let this = Self {
            strom_consensus_event,
            validator,
            core,
            subscribers: Vec::new(),
            command: stream,
            network,
            orderpool,
            canonical_block_stream
        };

        tp.spawn_critical("consensus", this.boxed());

        ConsensusHandle { sender: tx }
    }

    fn on_command(&mut self, command: ConsensusCommand) {
        match command {
            ConsensusCommand::Subscribe(sender) => self.subscribers.push(sender)
        }
    }
}

impl<OrderPool, Validator> Future for ConsensusManager<OrderPool, Validator>
where
    OrderPool: OrderPoolHandle,
    Validator: BundleValidator
{
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut work = 10;

        loop {
            while let Poll::Ready(Some(msg)) = self.command.poll_next_unpin(cx) {
                self.on_command(msg)
            }

            if let Poll::Ready(msg) = self.core.poll_next_unpin(cx).filter_map(|item| {
                item.transpose()
                    .map_err(|e| {
                        warn!(?e, "consensus error");
                        e
                    })
                    .ok()
                    .flatten()
            }) {
                self.subscribers
                    .retain_mut(|sub| sub.try_send(msg.clone()).is_ok());
            }

            work -= 1;
            if work == 0 {
                cx.waker().wake_by_ref();
                return Poll::Pending
            }
        }
    }
}

pub enum ConsensusCommand {
    Subscribe(Sender<ConsensusMessage>)
}

#[derive(Debug, Clone)]
pub struct ConsensusHandle {
    pub sender: Sender<ConsensusCommand>
}

impl ConsensusUpdater for ConsensusHandle {
    fn new_block(&self, _block: ()) {
        todo!()
    }
}

impl ConsensusListener for ConsensusHandle {
    fn subscribe_messages(&self) -> Pin<Box<dyn Stream<Item = ConsensusMessage>>> {
        let (tx, rx) = channel(10);
        let _ = self.sender.try_send(ConsensusCommand::Subscribe(tx));

        Box::pin(ReceiverStream::new(rx))
    }
}
