use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    marker::PhantomData,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker},
    time::Duration
};

use alloy::{
    primitives::{BlockNumber, Bytes},
    providers::{network::Network, Provider},
    transports::Transport
};
use angstrom_metrics::ConsensusMetricsWrapper;
use angstrom_network::{manager::StromConsensusEvent, StromMessage};
use angstrom_types::{
    consensus::{PreProposal, Proposal},
    contract_payloads::angstrom::AngstromBundle,
    orders::{OrderSet, PoolSolution},
    primitive::PeerId,
    sol_bindings::{
        grouped_orders::{GroupedVanillaOrder, OrderWithStorageData},
        rpc_orders::TopOfBlockOrder
    }
};
use angstrom_utils::timer::async_time_fn;
use futures::{future::BoxFuture, Future, Stream, StreamExt};
use itertools::Itertools;
use matching_engine::MatchingManager;
use order_pool::order_storage::OrderStorage;
use serde::{Deserialize, Serialize};
use tokio::time;

use crate::{AngstromValidator, Signer};

async fn build_proposal(pre_proposals: Vec<PreProposal>) -> Result<Vec<PoolSolution>, String> {
    let matcher = MatchingManager {};
    matcher.build_proposal(pre_proposals).await
}

const INITIAL_STATE_DURATION: Duration = Duration::from_secs(3);

pub struct RoundStateMachine {
    current_state:          ConsensusState,
    signer:                 Signer,
    round_leader:           PeerId,
    validators:             Vec<AngstromValidator>,
    order_storage:          Arc<OrderStorage>,
    initial_state_duration: Duration,
    metrics:                ConsensusMetricsWrapper,
    transition_future:      Option<BoxFuture<'static, ConsensusState>>,
    initial_state_timer:    Option<Pin<Box<time::Sleep>>>,
    waker:                  Option<Waker>
}

impl RoundStateMachine {
    pub fn new(
        block_height: BlockNumber,
        order_storage: Arc<OrderStorage>,
        signer: Signer,
        round_leader: PeerId,
        validators: Vec<AngstromValidator>,
        metrics: ConsensusMetricsWrapper
    ) -> Self {
        let timer = Box::pin(time::sleep(INITIAL_STATE_DURATION));
        Self {
            current_state: Self::initial_state(block_height),
            round_leader,
            validators,
            initial_state_duration: INITIAL_STATE_DURATION,
            order_storage,
            signer,
            metrics,
            transition_future: None,
            initial_state_timer: Some(timer),

            waker: None /* provider,
                         * _phantom: PhantomData, */
        }
    }

    pub fn my_id(&self) -> PeerId {
        self.signer.my_id
    }

    pub fn is_leader(&self, node: PeerId) -> bool {
        self.round_leader == node
    }

    pub fn i_am_leader(&self) -> bool {
        self.is_leader(self.my_id())
    }

    pub fn has_quorum(&self, voters: usize) -> bool {
        voters > (self.validators.len() * 2) / 3
    }

    pub fn reset_round(&mut self, block: BlockNumber, leader: PeerId) {
        self.round_leader = leader;
        self.current_state = Self::initial_state(block);
        self.initial_state_timer = Some(Box::pin(time::sleep(self.initial_state_duration)));
        self.transition_future = None;
    }

    pub fn initial_state(block_height: BlockNumber) -> ConsensusState {
        ConsensusState::BidSubmission(BidSubmission { block_height, ..Default::default() })
    }

    pub fn my_pre_proposal(&self, pre_proposals: &HashSet<PreProposal>) -> Option<StromMessage> {
        pre_proposals
            .iter()
            .find(|p| p.source == self.my_id())
            .cloned()
            .map(StromMessage::PrePropose)
    }

    /// Invariants:
    ///  * won't be called with messages with payload from ourselves
    ///  * won't be called with messages for non-current block height
    pub fn on_strom_message(
        &mut self,
        strom_msg: StromConsensusEvent
    ) -> Option<(Option<PeerId>, StromMessage)> {
        let i_am_leader = self.i_am_leader();
        match strom_msg {
            StromConsensusEvent::PreProposal(_, pre_proposal) => {
                // we do not want to allow another node to push us to transition
                if !matches!(self.current_state, ConsensusState::BidAggregation(_)) {
                    return None;
                }

                if !pre_proposal.is_valid() {
                    return None;
                }

                if !i_am_leader {
                    let block_height = self.current_state.block_height();
                    let merged_pre_proposal = Self::generate_our_merged_pre_proposal(
                        block_height,
                        Vec::new(),
                        Vec::new(),
                        self.current_state.pre_proposals(),
                        &self.signer
                    );
                    self.current_state
                        .pre_proposals_mut()
                        .insert(merged_pre_proposal.clone());
                    let pre_proposals = self.current_state.pre_proposals();

                    if self.have_quorum(self.all_searcher_orders(pre_proposals))
                        && self.have_quorum(self.all_limit_orders(pre_proposals))
                    {
                        // send the quorum pre_proposal to the leader
                        return Some((
                            Some(self.round_leader.clone()),
                            StromMessage::PrePropose(merged_pre_proposal)
                        ));
                    }

                    return Some((None, StromMessage::PrePropose(merged_pre_proposal)));
                }

                // Leader path
                self.current_state.pre_proposals_mut().insert(pre_proposal);
                let pre_proposals = self.current_state.pre_proposals();
                let block_height = self.current_state.block_height();

                if self.have_quorum(self.all_searcher_orders(pre_proposals))
                    && self.have_quorum(self.all_limit_orders(pre_proposals))
                {
                    self.force_transition(ConsensusState::Finalization(Finalization {
                        block_height,
                        proposal: None,
                        pre_proposals: pre_proposals.clone()
                    }));
                    return None;
                }
            }
            StromConsensusEvent::Proposal(msg_sender, proposal) => {
                let Proposal {
                    source: proposal_sender, block_height: proposal_block_height, ..
                } = proposal;

                let pre_proposals = self.current_state.pre_proposals();
                if proposal.is_valid() && !i_am_leader {
                    self.force_transition(ConsensusState::Finalization(Finalization {
                        block_height:  proposal_block_height,
                        proposal:      Some(proposal),
                        pre_proposals: pre_proposals.clone()
                    }));
                }

                if i_am_leader {
                    panic!(
                        "Message sender: {}, Proposal block height: {}, Proposal source: {}, \
                         Current state {}, something is terribly wrong",
                        msg_sender,
                        proposal_block_height,
                        proposal_sender,
                        self.current_state.name()
                    );
                }
            }
        }

        None
    }

    fn generate_bid_aggregation(
        &self,
        block_height: BlockNumber,
        pre_proposals: &HashSet<PreProposal>
    ) -> BidAggregation {
        let OrderSet { limit, searcher } = self.order_storage.get_all_orders();
        let mut pre_proposals = pre_proposals.clone();

        let pre_proposal = Self::generate_our_merged_pre_proposal(
            block_height,
            limit,
            searcher,
            &pre_proposals,
            &self.signer
        );
        pre_proposals.insert(pre_proposal);

        BidAggregation { block_height, pre_proposals }
    }

    fn generate_our_merged_pre_proposal(
        block_height: BlockNumber,
        limit: Vec<OrderWithStorageData<GroupedVanillaOrder>>,
        searcher: Vec<OrderWithStorageData<TopOfBlockOrder>>,
        pre_proposals: &HashSet<PreProposal>,
        signer: &Signer
    ) -> PreProposal {
        let merged_limit_orders: Vec<_> = pre_proposals
            .iter()
            .flat_map(|p| p.limit.clone())
            .chain(limit)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        let merged_searcher_orders: Vec<_> = pre_proposals
            .iter()
            .flat_map(|p| p.searcher.clone())
            .chain(searcher)
            .into_group_map_by(|order| order.pool_id)
            .into_values()
            .filter_map(|group| group.into_iter().max_by_key(|order| order.tob_reward))
            .collect();

        PreProposal::generate_pre_proposal(
            block_height,
            signer.my_id,
            merged_limit_orders,
            merged_searcher_orders,
            &signer.key
        )
    }

    fn all_searcher_orders(
        &self,
        pre_proposals: &HashSet<PreProposal>
    ) -> Vec<OrderWithStorageData<TopOfBlockOrder>> {
        pre_proposals
            .iter()
            .flat_map(|pre_proposal| pre_proposal.searcher.clone())
            .collect()
    }

    fn all_limit_orders(
        &self,
        pre_proposals: &HashSet<PreProposal>
    ) -> Vec<OrderWithStorageData<GroupedVanillaOrder>> {
        pre_proposals
            .iter()
            .flat_map(|pre_proposal| pre_proposal.limit.clone())
            .collect()
    }

    fn have_quorum<T: Hash + Eq + Clone>(&self, orders: Vec<OrderWithStorageData<T>>) -> bool {
        orders.len() == self.filter_quorum_orders(orders).len()
    }

    fn filter_quorum_orders<T: Hash + Eq + Clone>(
        &self,
        input: Vec<OrderWithStorageData<T>>
    ) -> Vec<OrderWithStorageData<T>> {
        input
            .into_iter()
            .fold(HashMap::new(), |mut acc, order| {
                *acc.entry(order).or_insert(0) += 1;
                acc
            })
            .into_iter()
            .filter(|(_, count)| self.has_quorum(*count))
            .map(|(order, _)| order)
            .collect()
    }

    fn force_transition(&mut self, mut new_state: ConsensusState) {
        let signer = self.signer.clone();
        let metrics = self.metrics.clone();
        let pre_proposal_height = self.current_state.block_height();
        let pre_proposals: Vec<PreProposal> =
            self.current_state.pre_proposals().iter().cloned().collect();

        self.transition_future = Some(Box::pin(async move {
            if let ConsensusState::Finalization(finalization) = &mut new_state {
                // someone already proposed and we are not a leader
                if finalization.proposal.is_some() {
                    // TODO: use this opportunity to trigger the proposal validation
                    return new_state;
                }

                let (proposal_result, timer) = async_time_fn(|| async {
                    match build_proposal(pre_proposals.clone()).await {
                        Ok(solutions) => {
                            let proposal =
                                signer.sign_proposal(pre_proposal_height, pre_proposals, solutions);
                            Ok(proposal)
                        }
                        Err(err) => Err(err)
                    }
                })
                .await;
                metrics.set_proposal_build_time(pre_proposal_height, timer);

                match proposal_result {
                    Ok(proposal) => {
                        finalization.proposal = Some(proposal.clone());
                        // TODO: use the actual pools
                        let pools = HashMap::new();
                        let bundle = AngstromBundle::from_proposal(&proposal, &pools).unwrap();
                    }
                    Err(err) => {
                        // Handle the error from build_proposal
                        tracing::error!(
                            error = %err,
                            block_height = pre_proposal_height,
                            "Failed to build proposal"
                        );
                    }
                }
            }
            new_state
        }));

        // wake up the poller
        if let Some(waker) = &self.waker {
            waker.wake_by_ref();
        }
    }
}

impl Stream for RoundStateMachine {
    type Item = ConsensusState;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        this.waker = Some(cx.waker().clone());

        if let Some(future) = &mut this.transition_future {
            return match future.as_mut().poll(cx) {
                Poll::Ready(new_state) => Poll::Ready(Some(new_state)),
                Poll::Pending => Poll::Pending
            };
        }

        if let Some(timer) = &mut this.initial_state_timer {
            if timer.as_mut().poll(cx).is_ready() {
                if let ConsensusState::BidSubmission(BidSubmission {
                    block_height,
                    pre_proposals
                }) = &this.current_state
                {
                    let bid_aggregation =
                        this.generate_bid_aggregation(*block_height, pre_proposals);
                    this.transition_future =
                        Some(Box::pin(async { ConsensusState::BidAggregation(bid_aggregation) }));
                    this.initial_state_timer = None;
                }
            }
        }

        Poll::Pending
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BidSubmission {
    pub block_height:  BlockNumber,
    // this is used mostly for early messages
    pub pre_proposals: HashSet<PreProposal>
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BidAggregation {
    pub block_height:  BlockNumber,
    pub pre_proposals: HashSet<PreProposal>
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Finalization {
    pub block_height:  BlockNumber,
    pub pre_proposals: HashSet<PreProposal>,
    pub proposal:      Option<Proposal>
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ConsensusState {
    BidSubmission(BidSubmission),
    BidAggregation(BidAggregation),
    Finalization(Finalization)
}

impl ConsensusState {
    fn name(&self) -> &'static str {
        match self {
            Self::BidSubmission(_) => "BidSubmission",
            Self::BidAggregation(_) => "BidAggregation",
            Self::Finalization(_) => "Finalization"
        }
    }
}

impl ConsensusState {
    pub fn block_height(&self) -> BlockNumber {
        match self {
            Self::BidSubmission(state) => state.block_height,
            Self::BidAggregation(state) => state.block_height,
            Self::Finalization(state) => state.block_height
        }
    }

    pub fn pre_proposals_mut(&mut self) -> &mut HashSet<PreProposal> {
        match self {
            Self::BidSubmission(state) => &mut state.pre_proposals,
            Self::BidAggregation(state) => &mut state.pre_proposals,
            Self::Finalization(state) => &mut state.pre_proposals
        }
    }

    pub fn pre_proposals(&self) -> &HashSet<PreProposal> {
        match self {
            Self::BidSubmission(state) => &state.pre_proposals,
            Self::BidAggregation(state) => &state.pre_proposals,
            Self::Finalization(state) => &state.pre_proposals
        }
    }
}
