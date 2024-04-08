use std::{
    pin::Pin,
    task::{Context, Poll}
};

use angstrom_types::submission::BestBundles;
use futures::FutureExt;
use guard_utils::{ConsensusState, COMMIT, PROPOSE};

use super::{
    commit::CommitState, propose::ProposeState, GlobalStateContext, RoundAction, RoundStateMessage,
    StateTransition, Timeout
};

/// Given we have pre-proposed. we now wait the Timeout
/// before transitioning to the next state
pub struct PreProposeState {
    timeout:          Timeout,
    commited_details: BestBundles
}
impl PreProposeState {
    pub fn new(timeout: Timeout, commited_details: BestBundles) -> Self {
        Self { timeout, commited_details }
    }
}

impl StateTransition for PreProposeState {
    fn should_transition(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        gs_context: GlobalStateContext
    ) -> Poll<(RoundAction, ConsensusState, Option<RoundStateMessage>)> {
        self.timeout.poll_unpin(cx).map(|_| {
            if gs_context.is_leader.is_leader() {
                (
                    RoundAction::Propose(ProposeState::new(
                        cx.waker().clone(),
                        self.commited_details.clone()
                    )),
                    PROPOSE,
                    None
                )
            } else {
                (
                    RoundAction::Commit(CommitState::new(
                        cx.waker().clone(),
                        // TODO: make this cleaner
                        self.commited_details.vanilla.take().unwrap()
                    )),
                    COMMIT,
                    None
                )
            }
        })
    }
}
