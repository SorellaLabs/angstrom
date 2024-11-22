use std::{
    collections::HashSet,
    pin::Pin,
    task::{Context, Poll, Waker}
};

use alloy::providers::Provider;
use angstrom_network::manager::StromConsensusEvent;
use angstrom_types::consensus::Proposal;
use futures::{Future, FutureExt};
use matching_engine::MatchingEngineHandle;

use super::{ConsensusState, SharedRoundState};

/// The finalization state.
///
/// At this point we verify the proposal that was sent. Once slashing is added,
/// we will have a fork here (higher level module will shove this state machine
/// off) where we will wait for proposals to be propagated (consensus states you
/// have a day max). in which they will be verified and the round will
/// officially close.
pub struct FinalizationState {
    verification_future: Pin<Box<dyn Future<Output = bool> + Send>>,
    completed:           bool
}

impl FinalizationState {
    pub fn new<P, Matching>(
        proposal: Proposal,
        handles: &mut SharedRoundState<P, Matching>,
        waker: Waker
    ) -> Self
    where
        P: Provider + 'static,
        Matching: MatchingEngineHandle
    {
        let preproposal = proposal
            .preproposals()
            .clone()
            .into_iter()
            .collect::<HashSet<_>>();

        let future = handles
            .matching_engine_output(preproposal)
            .map(move |output| {
                let (solution, _) = output.unwrap();

                let mut proposal_solution = proposal.solutions.clone();
                proposal_solution.sort();

                let mut verification_solution = solution;
                verification_solution.sort();

                if !proposal_solution
                    .into_iter()
                    .zip(verification_solution)
                    .all(|(p, v)| p == v)
                {
                    tracing::error!(
                        "Violation DETECTED. in future this will be related to slashing"
                    );
                    return false
                }

                true
            })
            .boxed();

        waker.wake_by_ref();

        Self { verification_future: future, completed: false }
    }
}

impl<P, Matching> ConsensusState<P, Matching> for FinalizationState
where
    P: Provider + 'static,
    Matching: MatchingEngineHandle
{
    fn on_consensus_message(
        &mut self,
        _: &mut SharedRoundState<P, Matching>,
        _: StromConsensusEvent
    ) {
        // no messages consensus related matter at this point. is just waiting
        // to be reset.
    }

    fn poll_transition(
        &mut self,
        _: &mut SharedRoundState<P, Matching>,
        cx: &mut Context<'_>
    ) -> Poll<Option<Box<dyn ConsensusState<P, Matching>>>> {
        if self.completed {
            return Poll::Ready(None)
        }

        if let Poll::Ready(result) = self.verification_future.poll_unpin(cx) {
            tracing::info!(%result, "consensus result");
            self.completed = true;
            return Poll::Ready(None)
        }

        Poll::Pending
    }
}
