use std::{
    collections::HashSet,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker}
};

use alloy::{
    consensus::BlockHeader,
    primitives::{Address, BlockNumber},
    providers::Provider
};
use angstrom_metrics::ConsensusMetricsWrapper;
use angstrom_network::{StromMessage, StromNetworkHandle, manager::StromConsensusEvent};
use angstrom_types::{
    block_sync::BlockSyncConsumer, contract_payloads::angstrom::UniswapAngstromRegistry,
    mev_boost::MevBoostProvider, primitive::AngstromSigner
};
use futures::StreamExt;
use matching_engine::MatchingEngineHandle;
use order_pool::order_storage::OrderStorage;
use reth_metrics::common::mpsc::UnboundedMeteredReceiver;
use reth_provider::{CanonStateNotification, CanonStateNotifications};
use reth_tasks::shutdown::GracefulShutdown;
use tokio_stream::wrappers::BroadcastStream;
use uniswap_v4::uniswap::pool_manager::SyncedUniswapPools;

use crate::{
    AngstromValidator,
    leader_selection::WeightedRoundRobin,
    rounds::{ConsensusMessage, RoundStateMachine, SharedRoundState}
};

const MODULE_NAME: &str = "Consensus";

pub struct ConsensusManager<P, Matching, BlockSync> {
    current_height:         BlockNumber,
    leader_selection:       WeightedRoundRobin,
    consensus_round_state:  RoundStateMachine<P, Matching>,
    canonical_block_stream: BroadcastStream<CanonStateNotification>,
    strom_consensus_event:  UnboundedMeteredReceiver<StromConsensusEvent>,
    network:                StromNetworkHandle,
    block_sync:             BlockSync,

    /// Track broadcasted messages to avoid rebroadcasting
    broadcasted_messages: HashSet<StromConsensusEvent>
}

impl<P, Matching, BlockSync> ConsensusManager<P, Matching, BlockSync>
where
    P: Provider + 'static,
    BlockSync: BlockSyncConsumer,
    Matching: MatchingEngineHandle
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        netdeps: ManagerNetworkDeps,
        signer: AngstromSigner,
        validators: Vec<AngstromValidator>,
        order_storage: Arc<OrderStorage>,
        current_height: BlockNumber,
        angstrom_address: Address,
        pool_registry: UniswapAngstromRegistry,
        uniswap_pools: SyncedUniswapPools,
        provider: MevBoostProvider<P>,
        matching_engine: Matching,
        block_sync: BlockSync
    ) -> Self {
        let ManagerNetworkDeps { network, canonical_block_stream, strom_consensus_event } = netdeps;
        let wrapped_broadcast_stream = BroadcastStream::new(canonical_block_stream);
        tracing::info!(?validators, "setting up with validators");
        let mut leader_selection = WeightedRoundRobin::new(validators.clone(), current_height);
        let leader = leader_selection.choose_proposer(current_height).unwrap();
        block_sync.register(MODULE_NAME);

        Self {
            strom_consensus_event,
            current_height,
            leader_selection,
            consensus_round_state: RoundStateMachine::new(SharedRoundState::new(
                current_height,
                angstrom_address,
                order_storage,
                signer,
                leader,
                validators.clone(),
                ConsensusMetricsWrapper::new(),
                pool_registry,
                uniswap_pools,
                provider,
                matching_engine
            )),
            block_sync,
            network,
            canonical_block_stream: wrapped_broadcast_stream,
            broadcasted_messages: HashSet::new()
        }
    }

    fn on_blockchain_state(&mut self, notification: CanonStateNotification, waker: Waker) {
        tracing::info!("got new block_chain state");
        let new_block = notification.tip();
        self.current_height = new_block.number();
        let round_leader = self
            .leader_selection
            .choose_proposer(self.current_height)
            .unwrap();
        tracing::info!(?round_leader, "selected new round leader");

        self.consensus_round_state
            .reset_round(self.current_height, round_leader);
        self.broadcasted_messages.clear();

        self.block_sync
            .sign_off_on_block(MODULE_NAME, self.current_height, Some(waker));
    }

    fn on_network_event(&mut self, event: StromConsensusEvent) {
        if self.current_height != event.block_height() {
            tracing::warn!(
                event_block_height=%event.block_height(),
                msg_sender=%event.sender(),
                current_height=%self.current_height,
                "ignoring event for wrong block",
            );
            return;
        }

        self.consensus_round_state.handle_message(event);
    }

    fn on_round_event(&mut self, event: ConsensusMessage) {
        match event {
            ConsensusMessage::PropagateProposal(p) => {
                self.network.broadcast_message(StromMessage::Propose(p))
            }
            ConsensusMessage::PropagatePreProposal(p) => {
                self.network.broadcast_message(StromMessage::PrePropose(p))
            }
            ConsensusMessage::PropagatePreProposalAgg(p) => self
                .network
                .broadcast_message(StromMessage::PreProposeAgg(p))
        }
    }

    pub async fn run_till_shutdown(mut self, sig: GracefulShutdown) {
        let mut g = None;
        tokio::select! {
            _ = &mut self => {
            }
            cancel = sig => {
                g = Some(cancel);
            }
        }

        // ensure we shutdown properly.
        if g.is_some() {
            self.cleanup().await;
        }

        drop(g);
    }

    /// Currently this doesn't do much. However,
    /// once we start adding slashing. this will be critical in storing
    /// all of our evidence.
    #[allow(unused)]
    async fn cleanup(mut self) {}

    pub fn current_leader(&self) -> Address {
        self.consensus_round_state.current_leader()
    }
}

impl<P, Matching, BlockSync> Future for ConsensusManager<P, Matching, BlockSync>
where
    P: Provider + 'static,
    Matching: MatchingEngineHandle,
    BlockSync: BlockSyncConsumer
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        while let Poll::Ready(Some(msg)) = this.canonical_block_stream.poll_next_unpin(cx) {
            match msg {
                Ok(notification) => this.on_blockchain_state(notification, cx.waker().clone()),
                Err(e) => tracing::error!("Error receiving chain state notification: {}", e)
            };
        }

        if this.block_sync.can_operate() {
            while let Poll::Ready(Some(msg)) = this.strom_consensus_event.poll_next_unpin(cx) {
                this.on_network_event(msg);
            }

            while let Poll::Ready(Some(msg)) = this.consensus_round_state.poll_next_unpin(cx) {
                this.on_round_event(msg);
            }
        }

        Poll::Pending
    }
}

pub struct ManagerNetworkDeps {
    network:                StromNetworkHandle,
    canonical_block_stream: CanonStateNotifications,
    strom_consensus_event:  UnboundedMeteredReceiver<StromConsensusEvent>
}

impl ManagerNetworkDeps {
    pub fn new(
        network: StromNetworkHandle,
        canonical_block_stream: CanonStateNotifications,
        strom_consensus_event: UnboundedMeteredReceiver<StromConsensusEvent>
    ) -> Self {
        Self { network, canonical_block_stream, strom_consensus_event }
    }
}
