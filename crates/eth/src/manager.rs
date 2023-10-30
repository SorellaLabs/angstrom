use std::{
    sync::Arc,
    task::{Context, Poll}
};

use common::PollExt;
use ethers_core::types::{Block, H256};
use ethers_flashbots::PendingBundleError;
use ethers_providers::{Middleware, PubsubClient, SubscriptionStream};
use futures::Future;
use futures_util::StreamExt;
use guard_types::submission::SubmissionBundle;
use tokio::sync::mpsc::{channel, Sender};
use tokio_stream::wrappers::ReceiverStream;

use crate::{
    handle::{EthCommand, EthHandle},
    relay_sender::RelaySender
};

pub enum EthNetworkEvent {
    NewBlock(Block<H256>),
    NewSyncBlock(Block<H256>),
    NewSubmission(Result<(), PendingBundleError>)
}

impl EthNetworkEvent {
    pub fn is_new_block(&self) -> bool {
        matches!(self, EthNetworkEvent::NewBlock(_))
    }

    pub fn is_sync_block(&self) -> bool {
        matches!(self, EthNetworkEvent::NewSyncBlock(_))
    }

    pub fn is_submission(&self) -> bool {
        matches!(self, EthNetworkEvent::NewSubmission(_))
    }
}

/// Holds all of our eth network state
pub struct EthNetworkManager<M: Middleware + 'static>
where
    <M as Middleware>::Provider: PubsubClient
{
    /// our command receiver
    _commander:       ReceiverStream<EthCommand>,
    /// people listening to events
    _event_listeners: Vec<Sender<EthNetworkEvent>>,

    /// for the leader to submit to relays
    relay_sender: RelaySender<M>,
    /// general new block stream. Will be updated when our local optimized
    /// mem-pool is built
    block_stream: SubscriptionStream<'static, M::Provider, Block<H256>>
}

impl<M: Middleware + 'static> EthNetworkManager<M>
where
    <M as Middleware>::Provider: PubsubClient
{
    pub async fn new(
        middleware: &'static M,
        relay_sender: RelaySender<M>
    ) -> anyhow::Result<EthHandle> {
        let block_stream = middleware.subscribe_blocks().await?;

        let (tx, rx) = channel(10);
        let stream = ReceiverStream::new(rx);

        let this =
            Self { relay_sender, block_stream, _commander: stream, _event_listeners: Vec::new() };
        let handle = EthHandle::new(tx);
        tokio::spawn(this);

        Ok(handle)
    }

    /// sends the bundle to all specified relays
    pub fn send_to_relay(&mut self, bundle: SubmissionBundle) {
        self.relay_sender.submit_bundle(bundle);
    }

    pub fn poll_block_stream(&mut self, cx: &mut Context<'_>) -> Poll<Block<H256>> {
        self.block_stream.poll_next_unpin(cx).filter_map(|f| f)
    }

    pub fn poll_relay_submission(
        &mut self,
        cx: &mut Context<'_>
    ) -> Poll<Result<(), PendingBundleError>> {
        self.relay_sender.poll(cx)
    }
}

impl<M: Middleware + 'static> Future for EthNetworkManager<M>
where
    <M as Middleware>::Provider: PubsubClient
{
    type Output = ();

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        todo!()
    }
}
