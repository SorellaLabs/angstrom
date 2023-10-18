use std::{
    ops::{Deref, DerefMut},
    pin::{pin, Pin},
    task::{Context, Poll},
    time::Duration
};

use common::{return_if, AtomicConsensus, ConsensusState, IsLeader, PollExt};
use futures::{Future, Stream, StreamExt};

use self::{
    commit::CommitState, order_accumulation::OrderAccumulationState, pre_propose::PreProposeState,
    propose::ProposeState, submit::SubmitState
};

pub mod commit;
pub mod order_accumulation;
pub mod pre_propose;
pub mod propose;
pub mod submit;

/// The current state and subsequent actions that should be taken
/// for such state in a given round. All state that this contains
/// is transient to the given ethereum block height
pub struct RoundState {
    /// The current ethereum height
    height:        u64,
    /// If this guard is leader for the given height
    is_leader:     IsLeader,
    /// global consensus state indicator
    consensus:     AtomicConsensus,
    /// the current action we should be taking at the moment of
    /// time for this height
    current_state: RoundAction
}

impl RoundState {
    pub fn new(height: u64, is_leader: IsLeader, consensus: AtomicConsensus) -> Self {
        Self {
            is_leader: is_leader.clone(),
            consensus,
            height,
            current_state: RoundAction::new(is_leader)
        }
    }

    pub fn new_height(&mut self, block_height: u64, is_leader: bool) {
        assert!(block_height > self.height);

        self.height = block_height;
        self.is_leader.set_leader(is_leader);
        self.consensus.reset();
        self.current_state = RoundAction::new(self.is_leader.clone());
    }

    pub fn current_height(&self) -> u64 {
        self.height
    }
}

impl Stream for RoundState {
    type Item = RoundStateMessage;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Poll::Ready((new_action, msg)) = self
            .current_state
            .poll_next_unpin(cx)
            .filter_map(|f| f)
            .map(|(round_action, new_state, message)| {
                self.consensus.update_state(new_state);
                (round_action, message)
            })
        {
            self.current_state = new_action;
            return_if!(msg => { is_some() } map(Poll::Ready));
        }

        return Poll::Pending
    }
}

pub enum RoundStateMessage {}

/// Should be on all different states of consensus. These trigger the moves
trait StateTransition {
    /// needs to pass context for timeout related tasks. returns the new round
    /// state with the updater for global consensus
    fn should_transition(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>
    ) -> Poll<(RoundAction, ConsensusState, Option<RoundStateMessage>)>;
}

#[repr(transparent)]
pub struct Timeout(Pin<Box<dyn Future<Output = ()>>>);

impl Deref for Timeout {
    type Target = Pin<Box<dyn Future<Output = ()>>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Timeout {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Timeout {
    pub fn new(timeout: Duration) -> Self {
        Timeout(Box::pin(async move { tokio::time::sleep(timeout).await }))
    }
}

/// Representation of a finite-state machine
pub enum RoundAction {
    /// The precommit state actions we
    OrderAccumulation(OrderAccumulationState),
    PrePropose(PreProposeState),
    Propose(ProposeState),
    Commit(CommitState),
    Submit(SubmitState)
}

impl RoundAction {
    pub fn new(is_leader: IsLeader) -> Self {
        Self::OrderAccumulation(OrderAccumulationState::new(
            // TODO: placeholder
            Timeout::new(Duration::from_secs(6)),
            is_leader
        ))
    }
}

impl Stream for RoundAction {
    type Item = (RoundAction, ConsensusState, Option<RoundStateMessage>);

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match &mut *self {
            RoundAction::OrderAccumulation(p) => Pin::new(p).should_transition(cx).map(|p| Some(p)),
            RoundAction::PrePropose(p) => Pin::new(p).should_transition(cx).map(|p| Some(p)),
            RoundAction::Propose(p) => Pin::new(p).should_transition(cx).map(|p| Some(p)),
            RoundAction::Commit(p) => Pin::new(p).should_transition(cx).map(|p| Some(p)),
            RoundAction::Submit(p) => Pin::new(p).should_transition(cx).map(|p| Some(p))
        }
    }
}
