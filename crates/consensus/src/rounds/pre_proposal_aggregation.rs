use std::{
    collections::HashSet,
    task::{Context, Poll, Waker},
    time::Instant
};

use alloy::providers::Provider;
use angstrom_network::manager::StromConsensusEvent;
use angstrom_types::consensus::{PreProposal, PreProposalAggregation, Proposal};
use matching_engine::MatchingEngineHandle;

use super::{ConsensusState, SharedRoundState};
use crate::rounds::{finalization::FinalizationState, proposal::ProposalState};

/// PreProposalAggregationState
///
/// The initialization of this state will take the 2/3 set of proposals this
/// node has seen. sign over them and then submit them to the network. This will
/// transition into finalization in two cases.
/// 1)
/// this node is the leader and receives 2/3 pre_proposals_aggregation ->
/// proposal state
/// 2) this node isn't leader and receives the proposal -> finalization
#[derive(Debug)]
pub struct PreProposalAggregationState {
    pre_proposals_aggregation: HashSet<PreProposalAggregation>,
    proposal:                  Option<Proposal>,
    trigger_time:              Instant,
    waker:                     Waker
}

impl PreProposalAggregationState {
    pub fn new<P, Matching>(
        pre_proposals: HashSet<PreProposal>,
        mut pre_proposals_aggregation: HashSet<PreProposalAggregation>,
        handles: &mut SharedRoundState<P, Matching>,
        trigger_time: Instant,
        waker: Waker
    ) -> Self
    where
        P: Provider + 'static,
        Matching: MatchingEngineHandle
    {
        // generate my pre_proposal aggregation
        let my_preproposal_aggregation = PreProposalAggregation::new(
            handles.block_height,
            &handles.signer,
            pre_proposals.into_iter().collect::<Vec<_>>()
        );

        // propagate my pre_proposal
        handles.propagate_message(my_preproposal_aggregation.clone().into());

        pre_proposals_aggregation.insert(my_preproposal_aggregation);

        // ensure we get polled to start the checks for when we have 2f +1 pre_proposals
        // collected
        waker.wake_by_ref();
        tracing::info!("starting pre proposal aggregation");

        Self { pre_proposals_aggregation, proposal: None, waker, trigger_time }
    }
}

impl<P, Matching> ConsensusState<P, Matching> for PreProposalAggregationState
where
    P: Provider + 'static,
    Matching: MatchingEngineHandle
{
    fn on_consensus_message(
        &mut self,
        handles: &mut SharedRoundState<P, Matching>,
        message: StromConsensusEvent
    ) {
        match message {
            StromConsensusEvent::PreProposal(..) => {
                tracing::debug!("got a lagging pre-proposal");
            }
            StromConsensusEvent::PreProposalAgg(peer_id, pre_proposal_agg) => handles
                .handle_pre_proposal_aggregation(
                    peer_id,
                    pre_proposal_agg,
                    &mut self.pre_proposals_aggregation
                ),
            StromConsensusEvent::Proposal(peer_id, proposal) => {
                if let Some(proposal) = handles.verify_proposal(peer_id, proposal) {
                    self.proposal = Some(proposal);
                    self.waker.wake_by_ref();
                }
            }
        }
    }

    fn poll_transition(
        &mut self,
        handles: &mut SharedRoundState<P, Matching>,
        cx: &mut Context<'_>
    ) -> Poll<Option<Box<dyn ConsensusState<P, Matching>>>> {
        // if we aren't the leader. we wait for the proposal to then verify in the
        // finalization state.
        if let Some(proposal) = self.proposal.take() {
            return Poll::Ready(Some(Box::new(FinalizationState::new(
                proposal,
                handles,
                cx.waker().clone()
            ))))
        }
        let cur_preproposals_aggs = self.pre_proposals_aggregation.len();
        let twthr = handles.two_thirds_of_validation_set();

        // if  we are the leader, then we will transition
        if cur_preproposals_aggs >= twthr && handles.i_am_leader() {
            tracing::info!(
                ?cur_preproposals_aggs,
                ?twthr,
                is_leader = handles.i_am_leader(),
                "aggregation transition to proposal"
            );
            return Poll::Ready(Some(Box::new(ProposalState::new(
                std::mem::take(&mut self.pre_proposals_aggregation),
                handles,
                self.trigger_time,
                cx.waker().clone()
            ))))
        }

        Poll::Pending
    }
}
