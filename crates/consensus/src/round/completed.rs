use std::{
    pin::Pin,
    task::{Context, Poll}
};

use angstrom_utils::ConsensusState;

use super::{GlobalStateContext, RoundAction, RoundStateMessage, StateTransition};

/// waiting for next block state. no consensus actions at this time
pub struct CompletedState;

impl StateTransition for CompletedState {
    fn should_transition(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _: GlobalStateContext
    ) -> Poll<(RoundAction, ConsensusState, Option<RoundStateMessage>)> {
        Poll::Pending
    }
}
