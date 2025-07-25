use std::{
    collections::HashSet,
    task::{Context, Poll, Waker},
    time::Instant
};

use alloy::providers::Provider;
use angstrom_types::{
    consensus::{
        ConsensusRoundName, PreProposal, PreProposalAggregation, Proposal, StromConsensusEvent
    },
    primitive::AngstromMetaSigner
};
use futures::FutureExt;
use matching_engine::MatchingEngineHandle;

use super::{
    ConsensusState, SharedRoundState, finalization::FinalizationState,
    pre_proposal::PreProposalState, preproposal_wait_trigger::PreProposalWaitTrigger
};

/// BidAggregationState
///
/// This is the first step in the state machine and is initialized on new
/// blocks. It will wait till the transition timeout triggers and then we will
/// go to pre_proposal.
#[derive(Debug)]
pub struct BidAggregationState {
    /// because the start is timeout based. We won't propagate our pre_proposal
    /// till the timeout occurs. However if we get one before then, we still
    /// want to hold onto it.
    received_pre_proposals:    HashSet<PreProposal>,
    /// we collect these here given that the leader could be running behind.
    pre_proposals_aggregation: HashSet<PreProposalAggregation>,
    proposal:                  Option<Proposal>,
    start_time:                Instant,
    transition_timeout:        PreProposalWaitTrigger,
    waker:                     Option<Waker>
}

impl BidAggregationState {
    pub fn new(transition_timeout: PreProposalWaitTrigger) -> Self {
        // let sleep = sleep(transition_timeout);
        tracing::info!("starting bid aggregation");

        Self {
            received_pre_proposals: HashSet::default(),
            pre_proposals_aggregation: HashSet::default(),
            transition_timeout,
            start_time: Instant::now(),
            proposal: None,
            waker: None
        }
    }
}

impl<P, Matching, S> ConsensusState<P, Matching, S> for BidAggregationState
where
    P: Provider + Unpin + 'static,
    Matching: MatchingEngineHandle,
    S: AngstromMetaSigner
{
    fn on_consensus_message(
        &mut self,
        handles: &mut SharedRoundState<P, Matching, S>,
        message: StromConsensusEvent
    ) {
        match message {
            StromConsensusEvent::PreProposal(_, pre_proposal) => {
                handles.handle_pre_proposal(pre_proposal, &mut self.received_pre_proposals);
            }
            StromConsensusEvent::PreProposalAgg(_, agg) => {
                handles.handle_pre_proposal_aggregation(agg, &mut self.pre_proposals_aggregation);
            }
            StromConsensusEvent::Proposal(_, proposal) => {
                if let Some(proposal) = handles.verify_proposal(proposal) {
                    // given a proposal was seen. we will skip directly to verification
                    self.proposal = Some(proposal);
                    self.waker.as_ref().inspect(|w| w.wake_by_ref());
                }
            }
            _ => {}
        }
    }

    fn poll_transition(
        &mut self,
        handles: &mut SharedRoundState<P, Matching, S>,
        cx: &mut Context<'_>
    ) -> Poll<Option<Box<dyn ConsensusState<P, Matching, S>>>> {
        self.waker = Some(cx.waker().clone());
        if let Some(proposal) = self.proposal.take() {
            // skip to finalization
            return Poll::Ready(Some(Box::new(FinalizationState::new(
                proposal,
                handles,
                cx.waker().clone()
            ))));
        }

        if self.transition_timeout.poll_unpin(cx).is_ready() {
            tracing::info!("transitioning out of order aggregation");
            // create the transition
            let pre_proposal = PreProposalState::new(
                handles.block_height,
                std::mem::take(&mut self.received_pre_proposals),
                std::mem::take(&mut self.pre_proposals_aggregation),
                handles,
                self.start_time,
                cx.waker().clone()
            );

            // return the transition
            return Poll::Ready(Some(Box::new(pre_proposal)));
        }

        Poll::Pending
    }

    fn name(&self) -> ConsensusRoundName {
        ConsensusRoundName::BidAggregation
    }
}
