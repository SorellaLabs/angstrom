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
use reth_provider::CanonStateNotifications;
use tokio::sync::mpsc::{channel, Sender};
use tokio_stream::wrappers::ReceiverStream;

use crate::{
    handle::{EthCommand, EthHandle},
    relay_sender::RelaySender
};

pub enum EthNetworkEvent {}

/// Holds all of our eth network state.
/// Will do the following
/// 1) Deal with submitting bundles
/// 2) Deal with fetching block state differences + fmt (need for validation and
///    orderpool)
pub struct EthNetworkManager<M: Middleware + 'static> {
    /// our command receiver
    commander:       ReceiverStream<EthCommand>,
    /// people listening to events
    event_listeners: Vec<Sender<EthNetworkEvent>>,

    /// for the leader to submit to relays
    relay_sender:      RelaySender<M>,
    /// Notifications for Canonical Block updates
    canonical_updates: CanonStateNotifications
}

impl<M: Middleware + 'static> EthNetworkManager<M> {
    pub fn new(
        canonical_updates: CanonStateNotifications,
        relay_sender: RelaySender<M>
    ) -> anyhow::Result<EthHandle> {
        let (tx, rx) = channel(10);
        let stream = ReceiverStream::new(rx);

        let this = Self {
            relay_sender,
            canonical_updates,
            commander: stream,
            event_listeners: Vec::new()
        };

        let handle = EthHandle::new(tx);
        tokio::spawn(this);

        Ok(handle)
    }

    /// sends the bundle to all specified relays
    pub fn send_to_relay(&mut self, bundle: SubmissionBundle) {
        self.relay_sender.submit_bundle(bundle);
    }

    pub fn poll_relay_submission(
        &mut self,
        cx: &mut Context<'_>
    ) -> Poll<Result<(), PendingBundleError>> {
        self.relay_sender.poll(cx)
    }
}

impl<M: Middleware + 'static> Future for EthNetworkManager<M> {
    type Output = ();

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        todo!()
    }
}
