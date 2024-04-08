use std::{
    pin::Pin,
    task::{Context, Poll, Waker}
};

use angstrom_types::{consensus::Proposal, primitive::Angstrom::Bundle};
use guard_utils::{ConsensusState, WAITING_NEXT_BLOCK};

use super::{
    completed::CompletedState, GlobalStateContext, RoundAction, RoundStateMessage, StateTransition
};
#[allow(dead_code)]
pub enum CommitVote {
    Commit(()),
    Nil(())
}
#[allow(dead_code)]
pub struct CommitState {
    /// This is specifically vanilla as this is the only bundle we care about
    /// on this state path
    best_bundle: Bundle,
    waker:       Waker,
    vote:        Option<CommitVote>
}

#[allow(dead_code)]
impl CommitState {
    pub fn new(waker: Waker, commited_bundle: Bundle) -> Self {
        Self { best_bundle: commited_bundle, waker, vote: None }
    }

    pub fn on_proposal(&mut self, _proposal: Proposal) {
        // some code here to check the proposal against our bundle
        // to make sure that the lower bound and other stuff is
        // up to standard.
        //
        // don't love this tho that there needs to be another poll to transition
        self.waker.wake_by_ref();
        todo!()
    }
}

impl StateTransition for CommitState {
    fn should_transition(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _: GlobalStateContext
    ) -> Poll<(RoundAction, ConsensusState, Option<RoundStateMessage>)> {
        if let Some(_vote) = self.vote.take() {
            return Poll::Ready((
                RoundAction::Completed(CompletedState),
                WAITING_NEXT_BLOCK,
                Some(RoundStateMessage::Commit())
            ))
        }
        Poll::Pending
    }
}
