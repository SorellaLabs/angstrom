use std::{
    ops::{Deref, DerefMut},
    pin::Pin,
    task::{Context, Poll},
    time::Duration
};

use angstrom_types::{
    consensus::{Commit, PreProposal, Proposal},
    submission::BestBundles
};
use futures::{Future, Stream};
use guard_utils::{return_if, AtomicConsensus, ConsensusState, IsLeader, ORDER_ACCUMULATION};
use reth_primitives::B512;

use self::{
    commit::CommitState, completed::CompletedState, order_accumulation::OrderAccumulationState,
    pre_propose::PreProposeState, propose::ProposeState, submit::SubmitState
};

pub mod commit;
pub mod completed;
pub mod order_accumulation;
pub mod pre_propose;
pub mod propose;
pub mod submit;

/// The current state and subsequent actions that should be taken
/// for such state in a given round. All state that this contains
/// is transient to the given ethereum block height
pub struct RoundState {
    /// The current ethereum height
    height:         u64,
    /// If this guard is leader for the given height
    is_leader:      IsLeader,
    /// The current leader address,
    leader_address: B512,
    /// global consensus state indicator
    consensus:      AtomicConsensus,
    /// the current action we should be taking at the moment of
    /// time for this height
    current_state:  RoundAction
}

impl RoundState {
    #[allow(dead_code)]
    pub fn new(
        height: u64,
        is_leader: IsLeader,
        consensus: AtomicConsensus,
        leader_address: B512
    ) -> Self {
        Self {
            is_leader: is_leader.clone(),
            consensus,
            height,
            current_state: RoundAction::new(is_leader),
            leader_address
        }
    }

    pub fn new_height(&mut self, block_height: u64, leader_address: B512, is_leader: bool) {
        assert!(block_height > self.height);

        self.height = block_height;
        self.is_leader.set_leader(is_leader);
        self.leader_address = leader_address;
        self.consensus.reset();
        self.current_state = RoundAction::new(self.is_leader.clone());
    }

    pub fn current_height(&self) -> u64 {
        self.height
    }

    #[allow(dead_code)]
    pub fn on_commit(&mut self, _commit: Commit) {
        todo!()
    }

    #[allow(dead_code)]
    pub fn on_proposal(&mut self, _proposal: Proposal) {
        todo!()
    }

    #[allow(dead_code)]
    pub fn on_pre_propose(&mut self, _pre_propose: PreProposal) {
        todo!()
    }

    #[allow(dead_code)]
    // will be updated to include the lower bound and other stuff
    pub fn new_best_details(&mut self, bundle_details: BestBundles) {
        let state = self.consensus.get_current_state();

        if self.is_leader.is_leader() || state == ORDER_ACCUMULATION {
            self.current_state.new_best_details(bundle_details);
        }
    }
}

impl Stream for RoundState {
    type Item = RoundStateMessage;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let is_leader = self.is_leader.clone();
        if let Poll::Ready((new_action, msg)) = Pin::new(&mut self.current_state)
            .should_transition(cx, GlobalStateContext { is_leader })
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

pub enum RoundStateMessage {
    /// All guards lock there lower-bound and broadcast it
    PrePropose(),
    /// the leader for this round will send out the vanilla bundle and
    /// lower-bound commit for the round
    Proposal(),
    /// the commit or nil vote the the lower-bound + vanilla proposal
    Commit(),
    /// if leader. then the finalized bundle that is sent to builders
    RelaySubmission()
}

pub struct GlobalStateContext {
    pub is_leader: IsLeader
}

/// Should be on all different states of consensus. These trigger the moves
trait StateTransition {
    /// needs to pass context for timeout related tasks. returns the new round
    /// state with the updater for global consensus
    fn should_transition(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        global_state: GlobalStateContext
    ) -> Poll<(RoundAction, ConsensusState, Option<RoundStateMessage>)>;
}

#[repr(transparent)]
pub struct Timeout(Pin<Box<dyn Future<Output = ()> + Send>>);

impl Deref for Timeout {
    type Target = Pin<Box<dyn Future<Output = ()> + Send>>;

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
    Submit(SubmitState),
    /// non leader completed state
    Completed(CompletedState)
}

impl RoundAction {
    pub fn new(is_leader: IsLeader) -> Self {
        Self::OrderAccumulation(OrderAccumulationState::new(
            // TODO: placeholder
            Timeout::new(Duration::from_secs(6)),
            is_leader
        ))
    }

    pub fn new_best_details(&mut self, _bundle_details: BestBundles) {
        todo!()
    }
}

impl StateTransition for RoundAction {
    fn should_transition(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        global_state: GlobalStateContext
    ) -> Poll<(RoundAction, ConsensusState, Option<RoundStateMessage>)> {
        match &mut *self {
            RoundAction::OrderAccumulation(p) => Pin::new(p).should_transition(cx, global_state),
            RoundAction::PrePropose(p) => Pin::new(p).should_transition(cx, global_state),
            RoundAction::Propose(p) => Pin::new(p).should_transition(cx, global_state),
            RoundAction::Commit(p) => Pin::new(p).should_transition(cx, global_state),
            RoundAction::Submit(p) => Pin::new(p).should_transition(cx, global_state),
            RoundAction::Completed(p) => Pin::new(p).should_transition(cx, global_state)
        }
    }
}
