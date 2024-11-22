use std::{
    collections::HashSet,
    task::{Context, Poll, Waker}
};

use alloy::{primitives::BlockNumber, providers::Provider, transports::Transport};
use angstrom_network::manager::StromConsensusEvent;
use angstrom_types::consensus::{PreProposal, PreProposalAggregation, Proposal};
use matching_engine::MatchingEngineHandle;

use super::{ConsensusState, SharedRoundState};
use crate::rounds::{
    finalization::FinalizationState, pre_proposal_aggregation::PreProposalAggregationState,
    ConsensusMessage
};

/// PreProposalState
///
/// This part of the consensus state machine initializes when the bid
/// aggregation phase ends and we generate + propagate our pre_proposal. This
/// part of the state machine transitions when we have hit 2/3 pre_proposals
/// collected. We then transition to pre_proposals_aggregation_state.
#[derive(Debug)]
pub struct PreProposalState {
    pre_proposals:             HashSet<PreProposal>,
    pre_proposals_aggregation: HashSet<PreProposalAggregation>,
    proposal:                  Option<Proposal>,
    waker:                     Waker
}

impl PreProposalState {
    pub fn new<P, Matching>(
        block_height: BlockNumber,
        mut pre_proposals: HashSet<PreProposal>,
        pre_proposals_aggregation: HashSet<PreProposalAggregation>,
        handles: &mut SharedRoundState<P, Matching>,
        waker: Waker
    ) -> Self
    where
        P: Provider + 'static,
        Matching: MatchingEngineHandle
    {
        // generate my pre_proposal
        let my_preproposal =
            PreProposal::new(block_height, &handles.signer, handles.order_storage.get_all_orders());

        // propagate my pre_proposal
        handles.propagate_message(ConsensusMessage::PropagatePreProposal(my_preproposal.clone()));

        pre_proposals.insert(my_preproposal);

        // ensure we get polled to start the checks for when we have 2f +1 pre_proposals
        // collected
        waker.wake_by_ref();

        Self { pre_proposals, pre_proposals_aggregation, proposal: None, waker }
    }
}

impl<P, Matching> ConsensusState<P, Matching> for PreProposalState
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
            StromConsensusEvent::PreProposal(peer_id, pre_proposal) => {
                handles.handle_pre_proposal(peer_id, pre_proposal, &mut self.pre_proposals);

                if self.pre_proposals.len() >= handles.two_thirds_of_validation_set() {
                    self.waker.wake_by_ref();
                }
            }
            StromConsensusEvent::PreProposalAgg(peer_id, pre_proposal_agg) => handles
                .handle_pre_proposal_aggregation(
                    peer_id,
                    pre_proposal_agg,
                    &mut self.pre_proposals_aggregation
                ),
            StromConsensusEvent::Proposal(peer_id, proposal) => {
                if let Some(proposal) = handles.verify_proposal(peer_id, proposal) {
                    // given a proposal was seen. we will skip directly to verification
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
        if let Some(proposal) = self.proposal.take() {
            // skip to finalization
            return Poll::Ready(Some(Box::new(FinalizationState::new(
                proposal,
                handles,
                cx.waker().clone()
            ))))
        }

        if self.pre_proposals.len() >= handles.two_thirds_of_validation_set() {
            return Poll::Ready(Some(Box::new(PreProposalAggregationState::new(
                std::mem::take(&mut self.pre_proposals),
                std::mem::take(&mut self.pre_proposals_aggregation),
                handles,
                cx.waker().clone()
            ))))
        }

        Poll::Pending
    }
}
