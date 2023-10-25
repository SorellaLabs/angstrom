use std::{
    pin::Pin,
    sync::mpsc::{Receiver, Sender},
    task::{Context, Poll}
};

use common::PollExt;
use futures::{Future, StreamExt};
use tracing::warn;

use crate::core::{ConsensusCore, ConsensusMessage};

pub struct ConsensusHandle {
    sender: Sender<ConsensusCommand>
}

pub struct ConsensusManager {
    core:        ConsensusCore,
    comands:     Receiver<ConsensusCommand>,
    subscribers: Vec<Sender<ConsensusMessage>>
}

impl Future for ConsensusManager {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.core.poll_next_unpin(cx).filter_map(|item| {
            item.transpose()
                .inspect_err(|e| warn!(?e, "consensus error"))
        })
    }
}

pub enum ConsensusCommand {}
