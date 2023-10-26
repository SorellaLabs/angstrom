use std::{
    pin::Pin,
    task::{Context, Poll}
};

use common::{ConsensusState, PollExt};
use ethers_core::types::{Block, H256};
use futures::{Future, Stream, StreamExt};
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio_stream::wrappers::ReceiverStream;
use tracing::warn;

use crate::{
    core::{ConsensusCore, ConsensusMessage},
    ConsensusListener, ConsensusUpdater
};

pub struct ConsensusManager {
    core: ConsensusCore,

    command:     ReceiverStream<ConsensusCommand>,
    subscribers: Vec<Sender<ConsensusMessage>>
}

impl ConsensusManager {
    pub async fn new() -> (ConsensusHandle, u64) {
        let (tx, rx) = channel(100);
        let stream = ReceiverStream::new(rx);
        let (core, bn) = ConsensusCore::new().await;

        let this = Self { core, subscribers: Vec::new(), command: stream };
        tokio::spawn(this);

        (ConsensusHandle { sender: tx }, bn)
    }

    fn on_command(&mut self, command: ConsensusCommand) {
        match command {
            ConsensusCommand::Subscribe(sender) => self.subscribers.push(sender)
        }
    }
}

impl Future for ConsensusManager {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut work = 10;

        loop {
            while let Poll::Ready(Some(msg)) = self.command.poll_next_unpin(cx) {
                self.on_command(msg)
            }

            if let Poll::Ready(msg) = self.core.poll_next_unpin(cx).filter_map(|item| {
                item.transpose()
                    .inspect_err(|e| warn!(?e, "consensus error"))
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
    sender: Sender<ConsensusCommand>
}

impl ConsensusListener for ConsensusHandle {
    fn subscribe_messages(&self) -> Pin<Box<dyn Stream<Item = ConsensusMessage>>> {
        let (tx, rx) = channel(10);
        let _ = self.sender.try_send(ConsensusCommand::Subscribe(tx));

        Box::pin(ReceiverStream::new(rx))
    }
}
