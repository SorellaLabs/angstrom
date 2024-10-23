use std::{collections::HashSet, marker::PhantomData, sync::Arc};

use alloy::{
    network::Network, primitives::BlockNumber, providers::Provider, transports::Transport
};
use angstrom_metrics::ConsensusMetricsWrapper;
use angstrom_network::{manager::StromConsensusEvent, StromMessage, StromNetworkHandle};
use futures::StreamExt;
use order_pool::order_storage::OrderStorage;
use reth_metrics::common::mpsc::UnboundedMeteredReceiver;
use reth_provider::{CanonStateNotification, CanonStateNotifications};
use tokio::select;
use tokio_stream::wrappers::BroadcastStream;

use crate::{
    leader_selection::WeightedRoundRobin,
    round::{BidAggregation, BidSubmission, ConsensusState, Finalization, RoundStateMachine},
    AngstromValidator, Signer
};

pub struct ConsensusManager<P, TR, N> {
    current_height:         BlockNumber,
    leader_selection:       WeightedRoundRobin,
    state_transition:       RoundStateMachine,
    canonical_block_stream: BroadcastStream<CanonStateNotification>,
    strom_consensus_event:  UnboundedMeteredReceiver<StromConsensusEvent>,
    network:                StromNetworkHandle,

    /// Track broadcasted messages to avoid rebroadcasting
    broadcasted_messages: HashSet<StromConsensusEvent>,
    _provider:            P,
    _phantom:             PhantomData<(TR, N)>
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

impl<P, TR, N> ConsensusManager<P, TR, N>
where
    P: Provider<TR, N> + Send + Sync,
    TR: Transport + Clone + Send + Sync,
    N: Network + Send + Sync
{
    pub fn new(
        netdeps: ManagerNetworkDeps,
        signer: Signer,
        validators: Vec<AngstromValidator>,
        order_storage: Arc<OrderStorage>,
        current_height: BlockNumber,
        _provider: P
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
                ConsensusMetricsWrapper::new()
            ),
            network,
            canonical_block_stream: wrapped_broadcast_stream,
            broadcasted_messages: HashSet::new(),
            _provider,
            _phantom: PhantomData
        }
    }

    async fn on_blockchain_state(&mut self, notification: CanonStateNotification) {
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

    async fn on_network_event(&mut self, event: StromConsensusEvent) {
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
            ConsensusState::BidSubmission(BidSubmission { .. }) => {}
            // means we transitioned from bid submission to aggregation, therefore we broadcast our
            // pre-proposal to the network
            ConsensusState::BidAggregation(BidAggregation { pre_proposals, .. }) => {
                self.network.broadcast_message(
                    self.state_transition
                        .my_pre_proposal(&pre_proposals)
                        .unwrap()
                );
            }
            // TODO: maybe trigger the round verification job after it has finished, if we are not a
            // leader
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
            ConsensusState::BidSubmission(BidSubmission { .. }) => {}
            ConsensusState::BidAggregation(BidAggregation { .. }) => {}
            ConsensusState::Finalization(Finalization { .. }) => {}
        }
    }

    pub async fn message_loop(mut self) {
        loop {
            select! {
                Some(msg) = self.canonical_block_stream.next() => {
                    match msg {
                        Ok(notification) => self.on_blockchain_state(notification).await,
                        Err(e) => tracing::error!("Error receiving chain state notification: {}", e)
                    };
                },
                Some(msg) = self.strom_consensus_event.next() => {
                    self.on_network_event(msg).await;
                },
                Some(new_state) = self.state_transition.next() => {
                    self.on_state_start(new_state);
                },
            }
        }
    }
}
