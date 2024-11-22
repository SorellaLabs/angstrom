use std::{
    collections::{HashMap, HashSet, VecDeque},
    hash::Hash,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration
};

use alloy::{
    primitives::{Address, BlockNumber, FixedBytes},
    providers::Provider,
    rpc::types::TransactionRequest
};
use angstrom_metrics::ConsensusMetricsWrapper;
use angstrom_network::manager::StromConsensusEvent;
use angstrom_types::{
    consensus::{PreProposal, PreProposalAggregation, Proposal},
    contract_payloads::angstrom::{BundleGasDetails, UniswapAngstromRegistry},
    matching::uniswap::PoolSnapshot,
    mev_boost::MevBoostProvider,
    orders::PoolSolution,
    primitive::{AngstromSigner, PeerId},
    sol_bindings::grouped_orders::OrderWithStorageData
};
use bid_aggregation::BidAggregationState;
use futures::{future::BoxFuture, FutureExt, Stream};
use itertools::Itertools;
use matching_engine::MatchingEngineHandle;
use order_pool::order_storage::OrderStorage;
use uniswap_v4::uniswap::pool_manager::SyncedUniswapPools;

use crate::AngstromValidator;

mod bid_aggregation;
mod finalization;
mod pre_proposal;
mod pre_proposal_aggregation;
mod proposal;

pub trait ConsensusState<P, Matching>: Send
where
    P: Provider,
    Matching: MatchingEngineHandle
{
    fn on_consensus_message(
        &mut self,
        handles: &mut SharedRoundState<P, Matching>,
        message: StromConsensusEvent
    );

    /// just like streams. Once this returns Poll::Ready(None). This consensus
    /// round is over
    fn poll_transition(
        &mut self,
        handles: &mut SharedRoundState<P, Matching>,
        cx: &mut Context<'_>
    ) -> Poll<Option<Box<dyn ConsensusState<P, Matching>>>>;
}

/// Holds and progresses the consensus state machine
pub struct RoundStateMachine<P, Matching> {
    current_state:           Box<dyn ConsensusState<P, Matching>>,
    /// for consensus, on a new block we wait a duration of time before signing
    /// our pre-proposal. this is the time
    consensus_wait_duration: Duration,
    shared_state:            SharedRoundState<P, Matching>
}

impl<P, Matching> RoundStateMachine<P, Matching>
where
    P: Provider + 'static,
    Matching: MatchingEngineHandle
{
    pub fn new(
        consensus_wait_duration: Duration,
        shared_state: SharedRoundState<P, Matching>
    ) -> Self {
        Self {
            current_state: Box::new(BidAggregationState::new(consensus_wait_duration)),
            consensus_wait_duration,
            shared_state
        }
    }

    pub fn reset_round(&mut self, new_block: u64, new_leader: PeerId) {
        self.shared_state.block_height = new_block;
        self.shared_state.round_leader = new_leader;

        self.current_state = Box::new(BidAggregationState::new(self.consensus_wait_duration));
    }

    pub fn handle_message(&mut self, event: StromConsensusEvent) {
        self.current_state
            .on_consensus_message(&mut self.shared_state, event);
    }
}

impl<P, Matching> Stream for RoundStateMachine<P, Matching>
where
    P: Provider,
    Matching: MatchingEngineHandle
{
    type Item = ConsensusMessage;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        if let Poll::Ready(Some(transitioned_state)) = this
            .current_state
            .poll_transition(&mut this.shared_state, cx)
        {
            this.current_state = transitioned_state;
        }

        if let Some(message) = this.shared_state.messages.pop_front() {
            return Poll::Ready(Some(message))
        }

        Poll::Pending
    }
}

pub struct SharedRoundState<P, Matching> {
    block_height:     BlockNumber,
    angstrom_address: Address,
    matching_engine:  Matching,
    signer:           AngstromSigner,
    round_leader:     PeerId,
    validators:       Vec<AngstromValidator>,
    order_storage:    Arc<OrderStorage>,
    _metrics:         ConsensusMetricsWrapper,
    pool_registry:    UniswapAngstromRegistry,
    uniswap_pools:    SyncedUniswapPools,
    provider:         Arc<MevBoostProvider<P>>,
    messages:         VecDeque<ConsensusMessage>
}

// contains shared impls
impl<P, Matching> SharedRoundState<P, Matching>
where
    P: Provider + 'static,
    Matching: MatchingEngineHandle
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        block_height: BlockNumber,
        angstrom_address: Address,
        order_storage: Arc<OrderStorage>,
        signer: AngstromSigner,
        round_leader: PeerId,
        validators: Vec<AngstromValidator>,
        metrics: ConsensusMetricsWrapper,
        pool_registry: UniswapAngstromRegistry,
        uniswap_pools: SyncedUniswapPools,
        provider: MevBoostProvider<P>,
        matching_engine: Matching
    ) -> Self {
        Self {
            block_height,
            angstrom_address,
            round_leader,
            validators,
            order_storage,
            pool_registry,
            uniswap_pools,
            signer,
            _metrics: metrics,
            matching_engine,
            messages: VecDeque::new(),
            provider: Arc::new(provider)
        }
    }

    fn propagate_message(&mut self, message: ConsensusMessage) {
        self.messages.push_back(message);
    }

    fn i_am_leader(&self) -> bool {
        self.round_leader == self.signer.id()
    }

    fn two_thirds_of_validation_set(&self) -> usize {
        (2 * self.validators.len()).div_ceil(3)
    }

    fn fetch_pool_snapshot(
        &self
    ) -> HashMap<FixedBytes<32>, (Address, Address, PoolSnapshot, u16)> {
        self.uniswap_pools
            .iter()
            .filter_map(|(key, pool)| {
                let (token_a, token_b, snapshot) =
                    pool.read().unwrap().fetch_pool_snapshot().ok()?;
                let entry = self.pool_registry.get_ang_entry(key)?;

                Some((*key, (token_a, token_b, snapshot, entry.store_index as u16)))
            })
            .collect::<HashMap<_, _>>()
    }

    fn matching_engine_output(
        &self,
        pre_proposal_aggregation: HashSet<PreProposalAggregation>
    ) -> BoxFuture<'static, eyre::Result<(Vec<PoolSolution>, BundleGasDetails)>> {
        // fetch
        let mut limit = Vec::new();
        let mut searcher = Vec::new();

        for pre_proposal_agg in pre_proposal_aggregation {
            pre_proposal_agg.pre_proposals.into_iter().for_each(|pre| {
                limit.extend(pre.limit);
                searcher.extend(pre.searcher);
            });
        }

        let limit = self.filter_quorum_orders(limit);
        let searcher = self.filter_quorum_orders(searcher);
        let pool_snapshots = self.fetch_pool_snapshot();

        let matcher = self.matching_engine.clone();

        async move { matcher.solve_pools(limit, searcher, pool_snapshots).await }.boxed()
    }

    fn filter_quorum_orders<O: Hash + Eq + Clone>(
        &self,
        input: Vec<OrderWithStorageData<O>>
    ) -> Vec<OrderWithStorageData<O>> {
        let two_thirds = self.two_thirds_of_validation_set();
        input
            .into_iter()
            .fold(HashMap::new(), |mut acc, order| {
                *acc.entry(order).or_insert(0) += 1;
                acc
            })
            .into_iter()
            .filter(|(_, count)| *count >= two_thirds)
            .map(|(order, _)| order)
            .collect()
    }

    fn handle_pre_proposal_aggregation(
        &mut self,
        peer_id: PeerId,
        pre_proposal_agg: PreProposalAggregation,
        pre_proposal_agg_set: &mut HashSet<PreProposalAggregation>
    ) {
        self.handle_proposal_verification(
            peer_id,
            pre_proposal_agg,
            pre_proposal_agg_set,
            |proposal, block| proposal.is_valid(block)
        )
    }

    fn verify_proposal(&mut self, peer_id: PeerId, proposal: Proposal) -> Option<Proposal> {
        if self.round_leader != peer_id {
            return None
        }

        proposal.is_valid(&self.block_height).then(|| {
            self.messages
                .push_back(ConsensusMessage::PropagateProposal(proposal.clone()));

            proposal
        })
    }

    fn handle_pre_proposal(
        &mut self,
        peer_id: PeerId,
        pre_proposal: PreProposal,
        pre_proposal_set: &mut HashSet<PreProposal>
    ) {
        self.handle_proposal_verification(
            peer_id,
            pre_proposal,
            pre_proposal_set,
            |proposal, block| proposal.is_valid(block)
        )
    }

    fn handle_proposal_verification<Pro>(
        &mut self,
        peer_id: PeerId,
        proposal: Pro,
        proposal_set: &mut HashSet<Pro>,
        valid: impl FnOnce(&Pro, &BlockNumber) -> bool
    ) where
        Pro: Into<ConsensusMessage> + Eq + Hash + Clone
    {
        if !self.validators.iter().map(|v| v.peer_id).contains(&peer_id) {
            tracing::warn!(peer=?peer_id,"got a consensus message from a invalid peer");
            return
        }
        // ensure pre_proposal is valid
        if !valid(&proposal, &self.block_height) {
            tracing::info!(peer=?peer_id,"got a invalid consensus message");
            return
        }

        // if  we don't have the pre_proposal, propagate it and then store it.
        // else log a message
        if !proposal_set.contains(&proposal) {
            self.propagate_message(proposal.clone().into());
            proposal_set.insert(proposal);
        } else {
            tracing::info!(peer=?peer_id,"got a duplicate consensus message");
        }
    }
}

/// These messages will only be broadcasted to the peer network if our consensus
/// contracts don't currently contain them.
#[derive(Debug, Clone)]
pub enum ConsensusMessage {
    PropagatePreProposal(PreProposal),
    PropagatePreProposalAgg(PreProposalAggregation),
    PropagateProposal(Proposal)
}

impl From<PreProposal> for ConsensusMessage {
    fn from(value: PreProposal) -> Self {
        Self::PropagatePreProposal(value)
    }
}

impl From<PreProposalAggregation> for ConsensusMessage {
    fn from(value: PreProposalAggregation) -> Self {
        Self::PropagatePreProposalAgg(value)
    }
}
