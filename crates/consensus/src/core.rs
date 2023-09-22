use std::task::Poll;

use guard_types::consensus::{Block, BlockId, Commit, EvidenceError, Proposal, Vote};
use sim::Simulator;
use thiserror::Error;

use crate::{
    bundle::{BundleVoteManager, ValidBundle},
    evidence::EvidenceCollector,
    executor::Executor,
    stage::Stage
};

/// The ConsensusCore module handles everything related to consensus.
/// This includes but not limited to.
///
/// 1) Collecting Evidence against misbehavior
/// 2) Verifying Block and bundle data
/// 3) Leader Selection
/// 4) Verifying Historical State
/// 5) Signing Votes, Commitments & Proposals
pub struct ConsensusCore<S: Simulator + 'static> {
    evidence_collector: EvidenceCollector,
    stage:              Stage,
    bundle_data:        BundleVoteManager,
    executor:           Executor<S>
}

#[derive(Debug, Clone)]
pub enum ConsensusMessage {
    PropagatePreVote(Vote),
    PropagateCommit(Commit),
    PropagateProposal(Proposal),
    /// new chain tip
    PropagateNewValidBlock(Block),
    /// Current round step
    NewRoundStep(NewRoundStep),
    /// Indicates that 2/3s is reached on block id
    VoteSetMaj23(VoteSetMaj23)
}

#[derive(Debug, Clone, Copy)]
pub struct NewRoundStep {
    pub height:                   i64,
    pub round:                    i32,
    pub step:                     u32,
    pub seconds_since_start_time: i64,
    pub last_commit_round:        i32
}

#[derive(Debug, Clone, Copy)]
pub struct VoteSetMaj23 {
    pub height:   i64,
    pub round:    i32,
    pub r#type:   i32,
    pub block_id: Option<BlockId>
}

#[derive(Debug, Error)]
pub enum ConsensusError {
    #[error("Evidence Module had an Error: {0#?}")]
    EvidenceError(#[from] EvidenceError)
}

impl<S: Simulator + 'static> ConsensusCore<S> {
    pub async fn new() -> Self {
        todo!()
    }

    fn on_executor(
        &mut self,
        executor_msg: Option<ExecutorMessage>
    ) -> Option<Result<ConsensusMessage, ConsensusError>> {
        let message = executor_msg?;
        None
    }
}

impl<S: Simulator + 'static> Stream for ConsensusCore<S> {
    type Item = Result<ConsensusMessage, ConsensusError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let stuff = self.executor.poll(cx);

        todo!()
    }
}
