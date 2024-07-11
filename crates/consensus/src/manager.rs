use std::{
    collections::HashSet,
    pin::Pin,
    task::{Context, Poll}
};

use angstrom_network::{manager::StromConsensusEvent, StromNetworkHandle};
use angstrom_types::consensus::Proposal;
use angstrom_utils::PollExt;
use bitmaps::Bitmap;
use futures::{Future, FutureExt, Stream, StreamExt};
use order_pool::OrderPoolHandle;
use reth_metrics::common::mpsc::UnboundedMeteredReceiver;
use reth_provider::CanonStateNotifications;
use reth_tasks::TaskSpawner;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio_stream::wrappers::ReceiverStream;
use tracing::warn;
use validation::BundleValidator;

use crate::{
    core::{ConsensusCore, ConsensusMessage},
    signer::Signer,
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

    signer:    Signer,
    orderpool: OrderPool,
    validator: Validator,

    canonical_proposal: Option<Proposal>,
    broadcast_cache:    HashSet<Bitmap<128>>
}

pub struct ManagerNetworkDeps {
    network:                StromNetworkHandle,
    canonical_block_stream: CanonStateNotifications,
    strom_consensus_event:  UnboundedMeteredReceiver<StromConsensusEvent>,
    tx:                     Sender<ConsensusCommand>,
    rx:                     Receiver<ConsensusCommand>
}

impl ManagerNetworkDeps {
    pub fn new(
        network: StromNetworkHandle,
        canonical_block_stream: CanonStateNotifications,
        strom_consensus_event: UnboundedMeteredReceiver<StromConsensusEvent>,
        tx: Sender<ConsensusCommand>,
        rx: Receiver<ConsensusCommand>
    ) -> Self {
        Self { network, canonical_block_stream, strom_consensus_event, tx, rx }
    }
}

impl<OrderPool, Validator> ConsensusManager<OrderPool, Validator>
where
    OrderPool: OrderPoolHandle,
    Validator: BundleValidator
{
    pub fn spawn<TP: TaskSpawner>(
        tp: TP,
        netdeps: ManagerNetworkDeps,
        signer: Signer,
        orderpool: OrderPool,
        validator: Validator
    ) -> ConsensusHandle {
        let ManagerNetworkDeps { network, canonical_block_stream, strom_consensus_event, tx, rx } =
            netdeps;
        let stream = ReceiverStream::new(rx);
        let (core, _bn) = ConsensusCore::new();

        let this = Self {
            strom_consensus_event,
            validator,
            core,
            subscribers: Vec::new(),
            command: stream,
            network,
            signer,
            orderpool,
            canonical_block_stream,
            canonical_proposal: None,
            broadcast_cache: HashSet::new()
        };

        tp.spawn_critical("consensus", this.boxed());

        ConsensusHandle { sender: tx }
    }

    fn on_command(&mut self, command: ConsensusCommand) {
        match command {
            ConsensusCommand::Subscribe(sender) => self.subscribers.push(sender)
        }
    }

    fn on_consensus_event(&mut self, command: StromConsensusEvent) {
        match command {
            // I think we're not worried about pre-proposal yet if we're going to be ignoring lower
            // bound
            StromConsensusEvent::PrePropose(..) => todo!(),
            StromConsensusEvent::Propose(_peer, proposal) => {
                // Validate the proposal itself - not currently checked
                proposal.validate(&[]);
                // Validate that the proposal contains the data we also think is the best
                // Also skipping this for now - maybe do this after we check proposal timing?

                // If we have an existing canonical proposal, and it's for the same or newer
                // block than our incoming proposal, we can reject the incoming
                // proposal
                if self
                    .canonical_proposal
                    .as_ref()
                    .is_some_and(|x| x.ethereum_block >= proposal.ethereum_block)
                {
                    // Our incoming proposal is old or bad
                    return
                }
                // Otherwise prepare our commit message and this proposal becomes our new
                // canonical proposal
                let commit = self
                    .signer
                    .sign_commit(proposal.ethereum_block, &proposal)
                    .unwrap(); // I don't think this can actually fail, validate
                self.canonical_proposal = Some(proposal);
                // Clear the broadcast cache because we have a new Proposal
                self.broadcast_cache = HashSet::new();

                // Broadcast our Commit message to everyone
                self.broadcast(ConsensusMessage::Commit(Box::new(commit)));
            }
            StromConsensusEvent::Commit(_, mut commit) => {
                // If we have no proposal than do we want to do anything here like hold on to
                // existing commits? Right now we're just going to leave here
                // and be done
                let Some(proposal) = self.canonical_proposal.as_ref() else {
                    return;
                };

                // Validate the commit itself - not currently checked
                commit.validate(&[]);

                // See if this commit is for our canonical proposal
                if commit.is_for(proposal) {
                    if commit.signed_by(self.signer.validator_id) {
                        // Do nothing I suppose, we're good.  Maybe we want to
                        // rebroadcast?
                    } else {
                        // Have we already processed a Commit message that had these signatures?
                        // Skipping similar messages can substantially cut
                        // down on chatter
                        if self.broadcast_cache.contains(commit.validator_map()) {
                            return
                        }
                        self.broadcast_cache.insert(*commit.validator_map());
                        // Add our signature to the commit and broadcast
                        commit.add_signature(self.signer.validator_id, &self.signer.key);
                        // Broadcast to our local subscribers
                        self.broadcast(ConsensusMessage::Commit(commit.clone()));
                        // Also broadcast to the Strom network
                        self.network
                            .broadcast_tx(angstrom_network::StromMessage::Commit(commit))
                    }
                }
            }
        }
    }

    fn broadcast(&mut self, msg: ConsensusMessage) {
        self.subscribers
            .retain_mut(|sub| sub.try_send(msg.clone()).is_ok());
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

            // We're handing one event per cycle instead of all events per cycle - might
            // want to change this depending on the priorty level of these
            // events compared to Command events
            if let Poll::Ready(Some(msg)) = self.strom_consensus_event.poll_next_unpin(cx) {
                self.on_consensus_event(msg);
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
                self.broadcast(msg.clone());
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
