use std::{
    collections::HashSet,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll}
};

use alloy::{primitives::BlockNumber, providers::Provider, transports::Transport};
use angstrom_metrics::ConsensusMetricsWrapper;
use angstrom_network::{manager::StromConsensusEvent, StromMessage, StromNetworkHandle};
use angstrom_types::contract_payloads::angstrom::UniswapAngstromRegistry;
use futures::StreamExt;
use matching_engine::MatchingEngineHandle;
use order_pool::order_storage::OrderStorage;
use reth_metrics::common::mpsc::UnboundedMeteredReceiver;
use reth_provider::{CanonStateNotification, CanonStateNotifications};
use tokio_stream::wrappers::BroadcastStream;
use uniswap_v4::uniswap::pool_manager::SyncedUniswapPools;

use crate::{
    leader_selection::WeightedRoundRobin,
    round_state::{
        ConsensusState, Finalization, PreProposalAggregation, PreProposalSubmission,
        RoundStateMachine
    },
    AngstromValidator, Signer
};

pub struct ConsensusManager<T, Matching> {
    current_height:         BlockNumber,
    leader_selection:       WeightedRoundRobin,
    state_transition:       RoundStateMachine<T, Matching>,
    canonical_block_stream: BroadcastStream<CanonStateNotification>,
    strom_consensus_event:  UnboundedMeteredReceiver<StromConsensusEvent>,
    network:                StromNetworkHandle,

    /// Track broadcasted messages to avoid rebroadcasting
    broadcasted_messages: HashSet<StromConsensusEvent>
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

impl<T, Matching> ConsensusManager<T, Matching>
where
    T: Transport + Clone,
    Matching: MatchingEngineHandle
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        netdeps: ManagerNetworkDeps,
        signer: Signer,
        validators: Vec<AngstromValidator>,
        order_storage: Arc<OrderStorage>,
        current_height: BlockNumber,
        pool_registry: UniswapAngstromRegistry,
        uniswap_pools: SyncedUniswapPools,
        provider: impl Provider<T> + 'static,
        matching_engine: Matching
    ) -> Self {
        let ManagerNetworkDeps { network, canonical_block_stream, strom_consensus_event } = netdeps;
        let wrapped_broadcast_stream = BroadcastStream::new(canonical_block_stream);
        let mut leader_selection = WeightedRoundRobin::new(validators.clone(), current_height);
        let leader = leader_selection.choose_proposer(current_height).unwrap();
        Self {
            strom_consensus_event,
            current_height,
            leader_selection,
            state_transition: RoundStateMachine::new(
                current_height,
                order_storage,
                signer,
                leader,
                validators.clone(),
                ConsensusMetricsWrapper::new(),
                pool_registry,
                uniswap_pools,
                provider,
                matching_engine
            ),
            network,
            canonical_block_stream: wrapped_broadcast_stream,
            broadcasted_messages: HashSet::new()
        }
    }

    fn on_blockchain_state(&mut self, notification: CanonStateNotification) {
        let new_block = notification.tip();
        self.current_height = new_block.block.number;
        let round_leader = self
            .leader_selection
            .choose_proposer(self.current_height)
            .unwrap();
        self.state_transition
            .reset_round(self.current_height, round_leader);
        self.broadcasted_messages.clear();
    }

    fn on_network_event(&mut self, event: StromConsensusEvent) {
        if self.current_height != event.block_height() {
            tracing::warn!(
                event_block_height=%event.block_height(),
                msg_sender=%event.sender(),
                current_height=%self.current_height,
                "ignoring event for wrong block",
            );
            return
        }

        if self.state_transition.my_id() == event.payload_source() {
            tracing::debug!(
                msg_sender=%event.sender(),
                block_heighth=%event.block_height(),
                message_type=%event.message_type(),
                "ignoring event that we sent to node",
            );
            return
        }

        if !self.broadcasted_messages.contains(&event) {
            self.network.broadcast_message(event.clone().into());
            self.broadcasted_messages.insert(event.clone());
        }

        if let Some((peer_id, msg)) = self.state_transition.on_strom_message(event.clone()) {
            if let Some(peer_id) = peer_id {
                self.network.send_message(peer_id, msg);
            } else {
                self.network.broadcast_message(msg);
            }
        }
    }

    pub fn on_state_start(&mut self, new_stat: ConsensusState) {
        match new_stat {
            // means we transitioned from commit phase to bid submission.
            // nothing much to do here. we just wait sometime to accumulate orders
            ConsensusState::PreProposalSubmission(PreProposalSubmission { .. }) => {}
            // means we transitioned from bid submission to aggregation, therefore we broadcast our
            // pre-proposal to the network
            ConsensusState::PreProposalAggregation(PreProposalAggregation {
                pre_proposals,
                ..
            }) => {
                self.network.broadcast_message(
                    self.state_transition
                        .my_pre_proposal(&pre_proposals)
                        .unwrap()
                );
            }
            // TODO: maybe trigger the round verification job after it has finished,
            // if we are not a leader
            ConsensusState::Finalization(finalization) => {
                // tell everyone what we sent out to Ethereum
                if self.state_transition.i_am_leader() {
                    self.network
                        .broadcast_message(StromMessage::Propose(finalization.proposal.unwrap()))
                }
            }
        }
    }

    pub fn on_state_end(&mut self, old_state: ConsensusState) {
        match old_state {
            ConsensusState::PreProposalSubmission(PreProposalSubmission { .. }) => {}
            ConsensusState::PreProposalAggregation(PreProposalAggregation { .. }) => {}
            ConsensusState::Finalization(Finalization { .. }) => {}
        }
    }
}

impl<T, Matching> Future for ConsensusManager<T, Matching>
where
    T: Transport + Clone,
    Matching: MatchingEngineHandle
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        if let Poll::Ready(Some(msg)) = this.canonical_block_stream.poll_next_unpin(cx) {
            match msg {
                Ok(notification) => this.on_blockchain_state(notification),
                Err(e) => tracing::error!("Error receiving chain state notification: {}", e)
            };
        }

        if let Poll::Ready(Some(msg)) = this.strom_consensus_event.poll_next_unpin(cx) {
            this.on_network_event(msg);
        }

        if let Poll::Ready(Some(new_state)) = this.state_transition.poll_next_unpin(cx) {
            match new_state {
                Ok(new_state) => this.on_state_start(new_state),
                Err(e) => {
                    tracing::error!("could not transition state: {}", e)
                }
            };
        }

        Poll::Pending
    }
}
