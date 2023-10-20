use std::{
    collections::HashMap,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll}
};

use action::{
    action_core::{ActionConfig, ActionCore, ActionMessage},
    RelaySender
};
use common::PollExt;
use consensus::{ConsensusCore, ConsensusMessage};
use ethers_flashbots::BroadcasterMiddleware;
use ethers_middleware::SignerMiddleware;
use ethers_providers::{Middleware, PubsubClient};
use futures::{Future, FutureExt};
use futures_util::StreamExt;
use guard_network::{GuardStakingEvent, NetworkConfig, PeerMessages, Swarm, SwarmEvent};
use guard_types::on_chain::{RawLvrSettlement, RawUserSettlement};
use sim::Simulator;
use tokio::sync::mpsc::{Sender, UnboundedSender};
use tracing::{debug, warn};
use url::Url;

use crate::{
    round_robin_sync::RoundRobinSync,
    submission_server::{
        Submission, SubmissionServer, SubmissionServerConfig, SubmissionServerInner,
        SubscriptionKind, SubscriptionResult
    },
    SourceMessages, Sources
};

// TODO: these values should be moved somewhere else bc there ugly
static SIMULATION_RELAY: &str = "https://relay.flashbots.net";
static BUILDER_URLS: &[&str] = &[
    "https://builder0x69.io",
    "https://rpc.beaverbuild.org",
    "https://relay.flashbots.net",
    "https://rsync-builder.xyz",
    "https://rpc.titanbuilder.xyz",
    "https://api.blocknative.com/v1/auction",
    "https://mev.api.blxrbdn.com",
    "https://eth-builder.com",
    "https://builder.gmbit.co/rpc",
    "https://buildai.net",
    "https://rpc.payload.de",
    "https://rpc.lightspeedbuilder.info",
    "https://rpc.nfactorial.xyz"
];

/// The control head of the Guard
pub struct Guard<M, S>
where
    M: Middleware + Unpin + 'static,
    S: Simulator + Unpin + 'static,
    <M as Middleware>::Provider: PubsubClient
{
    /// All of the sources that we read and write to
    sources:   Sources<M>,
    /// guard network connection
    consensus: ConsensusCore,
    /// deals with everything surrounding bundle building
    action:    ActionCore<S>,
    /// deals with round robin sycning
    syncing:   Option<RoundRobinSync<M>>
}

impl<M: Middleware + Unpin, S: Simulator + Unpin> Guard<M, S>
where
    <M as Middleware>::Provider: PubsubClient
{
    pub async fn new(
        middleware: &'static M,
        network_config: NetworkConfig,
        action_config: ActionConfig<S>,
        server_config: SubmissionServerConfig
    ) -> anyhow::Result<Self> {
        let sub_server = SubmissionServerInner::new(server_config).await?;
        let swarm = Swarm::new(network_config).await?;
        let relay_sender = RelaySender::new(Arc::new(SignerMiddleware::new(
            BroadcasterMiddleware::new(
                middleware,
                BUILDER_URLS
                    .into_iter()
                    .map(|u| Url::parse(u).unwrap())
                    .collect(),
                Url::parse(SIMULATION_RELAY)?,
                // TODO: move into own config from action
                action_config.bundle_key.clone()
            ),
            // TODO: move into on config from action
            action_config.edsca_key.clone()
        )));
        let sources = Sources::new(middleware, swarm, sub_server, relay_sender).await?;
        let action = ActionCore::new(action_config).await?;

        let (consensus, current_height) = ConsensusCore::new().await;
        let syncing = RoundRobinSync::new(middleware, current_height).await;

        Ok(Self { action, consensus, sources, syncing: Some(syncing) })
    }

    fn on_submission(&mut self, msg: Submission) {
        debug!(?msg, "handling new submission");

        match msg {
            Submission::ArbTx(arb_tx) => {
                self.action
                    .get_bundle_solver()
                    .new_searcher_transaction(arb_tx);
            }
            Submission::UserTx(user) => {
                self.action.get_bundle_solver().new_user_transaction(user);
            }
            Submission::Subscription(..) => {
                unreachable!("this is handled in the subscription server")
            }
        }
    }

    fn on_guard_net(&mut self, event: SwarmEvent) {
        debug!(?event, "handling network event");

        match event {
            SwarmEvent::ValidMessage { peer_id, request } => {
                debug!(?peer_id, ?request, "got data from peer");
                match request {
                    PeerMessages::PropagateBundle(bundle) => self
                        .action
                        .get_bundle_solver()
                        .new_bundle((*bundle).clone().into()),
                    PeerMessages::PropagateSearcherTransaction(tx) => {
                        self.action
                            .get_bundle_solver()
                            .new_searcher_transaction((*tx).clone().into());
                    }
                    PeerMessages::PropagateUserTransaction(tx) => {
                        self.action
                            .get_bundle_solver()
                            .new_user_transaction((*tx).clone().into());
                    }
                    PeerMessages::NewBlock(b) => {}
                    PeerMessages::BundleVote(vote) => {}
                    PeerMessages::Bundle23Vote(vote) => {}
                    PeerMessages::LeaderProposal(prop) => {}
                    PeerMessages::SignedLeaderProposal(signed_prop) => {}
                    PeerMessages::NewState(state) => {}
                }
            }
            res @ _ => {
                debug!(?res, "got swarm event");
            }
        }
    }

    fn on_action(&mut self, event: ActionMessage) {
        debug!(?event, "got actions");

        match event {
            ActionMessage::NewBestBundle(bundle) => {
                self.sources
                    .guard_net_mut()
                    .propagate_msg(PeerMessages::PropagateBundle(bundle.clone()));

                self.sources.on_new_best_bundle(bundle);
            }
            ActionMessage::NewValidUserTransaction(transaction) => {
                self.sources
                    .guard_net_mut()
                    .propagate_msg(PeerMessages::PropagateUserTransaction(transaction.clone()));

                self.sources
                    .on_new_user_txes(transaction.raw.order.clone().into());
            }
            ActionMessage::NewValidSearcherTransaction(transaction) => self
                .sources
                .guard_net_mut()
                .propagate_msg(PeerMessages::PropagateSearcherTransaction(transaction))
        }
    }

    fn on_consensus(&mut self, consensus: ConsensusMessage) {
        debug!(?consensus, "handling consensus event");
        todo!()
    }
}

impl<M: Middleware + Unpin, S: Simulator + Unpin> Future for Guard<M, S>
where
    <M as Middleware>::Provider: PubsubClient
{
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut work = 4096;
        loop {
            // poll all sources
            if let Poll::Ready(Some(sources)) = self.sources.poll_next_unpin(cx) {
                match sources {
                    SourceMessages::Swarm(swarm_event) => self.on_guard_net(swarm_event),
                    SourceMessages::RelaySubmission(relay_submission) => {
                        todo!()
                    }
                    SourceMessages::SubmissionServer(msg) => self.on_submission(msg),

                    SourceMessages::NewEthereumBlock(block) => {
                        let block = Arc::new(block);
                        if let Some(sync) = self.syncing.as_mut() {
                            sync.on_new_block(block);
                        }
                    }
                }
            }

            // poll actions
            self.action
                .poll(cx)
                .filter_map(|f| f)
                .apply(|msg| self.on_action(msg));

            // poll consensus
            self.consensus
                .poll_next_unpin(cx)
                .filter_map(|f| f.transpose().ok().flatten())
                .apply(|msg| self.on_consensus(msg));

            work -= 1;
            if work == 0 {
                cx.waker().wake_by_ref();
                return Poll::Pending
            }
        }
    }
}

#[cfg(feature = "test_harness")]
pub mod test_harness {
    use std::net::SocketAddr;

    use ethers_core::types::{H256, U64};
    use reth_primitives::PeerId;
    use tokio::{
        sync::{
            mpsc::{channel, Receiver},
            oneshot::{channel as one_channel, Sender as OneSender}
        },
        task::JoinHandle
    };

    use crate::guard::*;

    pub enum GuardCheatCodes {
        /// returns the address the peer is at
        PeerAddr(OneSender<SocketAddr>),
        /// makes the given peer id the leader
        MakeLeader(PeerId, U64),
        /// makes this node propagate there best bundle
        PropagateBundle,
        /// makes this node propagate all user txes it has
        PropagateUserTransactions,
        /// make the node propagate all searcher txes it has
        PropagateSearcherTransactions,
        /// checks to see if the guard contains a bundle we sent
        CheckForBundle(H256, OneSender<bool>),
        /// checks to see if the guard has a user transaction
        CheckForUserTx(H256, OneSender<bool>),
        /// check to see if the guard has a searcher tx
        CheckForSearcherTx(H256, OneSender<bool>)
    }

    pub struct GuardHandle {
        sender:        Sender<GuardCheatCodes>,
        _guard_handle: JoinHandle<()>
    }

    impl GuardHandle {
        pub async fn new<M, S>(
            middleware: &'static M,
            network_config: NetworkConfig,
            leader_config: ActionConfig<S>,
            server_config: SubmissionServerConfig
        ) -> anyhow::Result<Self>
        where
            M: Middleware + Unpin + 'static,
            S: Simulator + Unpin + 'static,
            <M as Middleware>::Provider: PubsubClient
        {
            let (tx, rx) = channel(10);
            let guard =
                Guard::new(middleware, network_config, leader_config, server_config).await?;
            let guard_wrapper = GuardWrapper { guard, receiver: rx };
            let handle = tokio::spawn(guard_wrapper);

            Ok(Self { sender: tx, _guard_handle: handle })
        }
    }

    impl GuardHandle {
        pub async fn get_addr(&self) -> SocketAddr {
            let (tx, rx) = one_channel();
            let _ = self.sender.send(GuardCheatCodes::PeerAddr(tx)).await;

            rx.await.unwrap()
        }

        pub async fn make_leader(&self, id: PeerId, block: U64) {
            let _ = self
                .sender
                .send(GuardCheatCodes::MakeLeader(id, block))
                .await;
        }

        pub async fn check_for_bundle(&self, hash: H256) -> bool {
            let (tx, rx) = one_channel();
            let _ = self
                .sender
                .send(GuardCheatCodes::CheckForBundle(hash, tx))
                .await;

            rx.await.unwrap()
        }

        pub async fn check_for_user_tx(&self, hash: H256) -> bool {
            let (tx, rx) = one_channel();
            let _ = self
                .sender
                .send(GuardCheatCodes::CheckForUserTx(hash, tx))
                .await;

            rx.await.unwrap()
        }

        pub async fn check_for_searcher_tx(&self, hash: H256) -> bool {
            let (tx, rx) = one_channel();
            let _ = self
                .sender
                .send(GuardCheatCodes::CheckForSearcherTx(hash, tx))
                .await;

            rx.await.unwrap()
        }

        pub async fn propagate_bundle(&self) {
            let _ = self.sender.send(GuardCheatCodes::PropagateBundle).await;
        }

        pub async fn propagate_user_transaction(&self) {
            let _ = self
                .sender
                .send(GuardCheatCodes::PropagateUserTransactions)
                .await;
        }

        pub async fn propagate_searcher_transaction(&self) {
            let _ = self
                .sender
                .send(GuardCheatCodes::PropagateSearcherTransactions)
                .await;
        }
    }

    pub struct GuardWrapper<M, S>
    where
        M: Middleware + Unpin + 'static,
        S: Simulator + Unpin + 'static,
        <M as Middleware>::Provider: PubsubClient
    {
        receiver: Receiver<GuardCheatCodes>,
        guard:    Guard<M, S>
    }

    impl<M, S> Future for GuardWrapper<M, S>
    where
        M: Middleware + Unpin + 'static,
        S: Simulator + Unpin + 'static,
        <M as Middleware>::Provider: PubsubClient
    {
        type Output = ();

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            if let Poll::Ready(msg) = self.receiver.poll_recv(cx) {
                let Some(msg) = msg else { return Poll::Ready(()) };

                match msg {
                    GuardCheatCodes::PeerAddr(sender) => {
                        let _ = sender.send(self.guard.network.local_addr());
                    }
                    GuardCheatCodes::MakeLeader(peer_id, block) => {
                        todo!()
                    }
                    GuardCheatCodes::CheckForBundle(hash, sender) => {
                        let _ = sender.send(self.guard.action.get_cow().check_for_bundle(hash));
                    }
                    GuardCheatCodes::CheckForUserTx(hash, sender) => {
                        let _ = sender.send(self.guard.action.get_cow().check_for_user_tx(hash));
                    }
                    GuardCheatCodes::CheckForSearcherTx(hash, sender) => {
                        let _ =
                            sender.send(self.guard.action.get_cow().check_for_searcher_tx(hash));
                    }
                    GuardCheatCodes::PropagateBundle => {
                        self.guard.action.get_cow().propagate_bundle();
                    }
                    GuardCheatCodes::PropagateUserTransactions => {
                        self.guard.action.get_cow().propagate_user_transactions();
                    }
                    GuardCheatCodes::PropagateSearcherTransactions => {
                        self.guard
                            .action
                            .get_cow()
                            .propagate_searcher_transactions();
                    }
                }
            }
            self.guard.poll_unpin(cx)
        }
    }
}
