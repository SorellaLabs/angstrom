use std::{
    collections::{HashMap, HashSet},
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    thread::current
};

use alloy_primitives::{bloom, BlockNumber};
use angstrom_metrics::ConsensusMetricsWrapper;
use angstrom_network::{manager::StromConsensusEvent, Peer, StromMessage, StromNetworkHandle};
use angstrom_types::{
    consensus::{Commit, PreProposal, Proposal},
    contract_payloads::angstrom::TopOfBlockOrder,
    orders::PoolSolution
};
use futures::{FutureExt, Stream, StreamExt};
use matching_engine::MatchingManager;
use order_pool::{order_storage::OrderStorage, timer::async_time_fn};
use reth_metrics::common::mpsc::UnboundedMeteredReceiver;
use reth_primitives::transaction::WithEncoded;
use reth_provider::{CanonStateNotification, CanonStateNotifications};
use reth_rpc_types::{beacon::relay::Validator, PeerId};
use reth_tasks::TaskSpawner;
use serde::__private::ser::FlatMapSerializeStructVariantAsMapValue;
use serde_json::error::Category::Data;
use tokio::{
    select,
    sync::mpsc::{channel, unbounded_channel, Receiver, Sender, UnboundedReceiver},
    task::{JoinHandle, JoinSet}
};
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};
use tracing::{error, warn};

use crate::{
    leader_selection::WeightedRoundRobin,
    round::{BidAggregation, BidSubmission, ConsensusState, Finalization, RoundStateMachine},
    AngstromValidator, ConsensusListener, ConsensusMessage, ConsensusUpdater, Signer
};

pub struct ConsensusManager {
    current_height:   BlockNumber,
    leader_selection: WeightedRoundRobin,
    state_transition: RoundStateMachine,

    /// keeps track of the current round state
    /// Used to trigger new consensus rounds
    canonical_block_stream: BroadcastStream<CanonStateNotification>,
    /// events from the network,
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

impl ConsensusManager {
    fn new(
        netdeps: ManagerNetworkDeps,
        signer: Signer,
        validators: Vec<AngstromValidator>,
        order_storage: Arc<OrderStorage>,
        current_height: BlockNumber
    ) -> Self {
        let ManagerNetworkDeps { network, canonical_block_stream, strom_consensus_event } = netdeps;
        let wrapped_broadcast_stream = BroadcastStream::new(canonical_block_stream);
        let mut leader_selection =
            WeightedRoundRobin::new(validators.clone(), current_height, None);
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
                None
            ),
            network,
            canonical_block_stream: wrapped_broadcast_stream,
            broadcasted_messages: HashSet::new()
        }
    }

    pub fn spawn<TP: TaskSpawner>(
        tp: TP,
        netdeps: ManagerNetworkDeps,
        signer: Signer,
        validators: Vec<AngstromValidator>,
        order_storage: Arc<OrderStorage>,
        current_height: BlockNumber
    ) -> JoinHandle<()> {
        let manager =
            ConsensusManager::new(netdeps, signer, validators, order_storage, current_height);
        let fut = manager.message_loop().boxed();
        tp.spawn_critical("consensus", fut)
    }

    async fn on_blockchain_state(&mut self, notification: CanonStateNotification) {
        let new_block = notification.tip();
        let new_block_height = new_block.block.number;

        if self.current_height + 1 == new_block_height {
            // TODO: remove the unwrap; if we go back or choose the same block it's a
            // problem
            let round_leader = self
                .leader_selection
                .choose_proposer(new_block_height)
                .unwrap();
            self.current_height = new_block_height;
            // TODO: does reset here make sense? Do we care what state we were in?
            // TODO: what if the transition does not complete before Ethereum fork-choices
            self.state_transition
                .reset_round(new_block_height, round_leader);
            self.broadcasted_messages.clear();
        } else {
            tracing::error!("Block height is not sequential, this breaks round robin!");
            panic!("Unrecoverable consensus error - Block height not sequential");
        }
    }

    async fn on_network_event(&mut self, event: StromConsensusEvent) {
        // TODO: do we want to ignore all messages for the wrong height?
        if self.current_height != event.block_height() {
            tracing::warn!(
                event_block_height=%event.block_height(),
                msg_sender=%event.sender(),
                current_height=%self.current_height,
                "ignoring event for wrong block",
            );
            return;
        }

        if self.state_transition.my_id() == event.payload_source() {
            tracing::debug!(
                msg_sender=%event.sender(),
                block_heighth=%event.block_height(),
                message_type=%event.message_type(),
                "ignoring event that we sent to node",
            );
            return;
        }

        // do we want to transition first and the send a message?
        if !self.broadcasted_messages.contains(&event) {
            self.network.broadcast_message(event.clone().into());
            self.broadcasted_messages.insert(event.clone());
        }

        if let Some(msg) = self.state_transition.on_strom_message(event.clone()) {
            self.network.broadcast_message(msg);
        }
    }

    pub fn on_state_start(&mut self, new_stat: ConsensusState) {
        match new_stat {
            // means we transitioned from commit phase to bid submission.
            // nothing much to do here. we just wait sometime to accumulate orders
            ConsensusState::BidSubmission(BidSubmission { pre_proposals, .. }) => {}
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
