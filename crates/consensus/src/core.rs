use std::{
    collections::VecDeque,
    pin::Pin,
    task::{Context, Poll}
};

use futures::{Stream, StreamExt};
use guard_types::{
    consensus::{
        Block, BundleVote, EvidenceError, GuardInfo, LeaderProposal, SignedLeaderProposal,
        Valid23Bundle
    },
    database::State,
    on_chain::SimmedBundle
};
use thiserror::Error;
use tracing::error;

use crate::{
    evidence::EvidenceCollector, round::RoundState, round_robin_algo::RoundRobinAlgo,
    signer::Signer
};

#[derive(Debug)]
pub enum ConsensusMessage {
    // voting related activities
    NewBundle23(Valid23Bundle),
    NewBundleVote(BundleVote),
    // finalization actions
    Proposal(LeaderProposal),
    SignedProposal(SignedLeaderProposal),
    // db related
    NewBlock(Block),
    NewBundle(SimmedBundle)
}

#[derive(Debug, Error)]
pub enum ConsensusError {
    #[error("Evidence Module had an Error: {0:#?}")]
    EvidenceError(#[from] EvidenceError)
}

/// The ConsensusCore module handles everything related to consensus.
/// This includes tracking slashable events, other guards commits and votes
/// and submitting to consensus.
/// # Design Principles
/// The main interfacing idea for the ConsensusCore is that this module
/// only operates on truths. What this means is this module avoids doing
/// any comparison, building or evaluation in order to keep it as simple as
/// possible (Of course we cannot rid all of this, however there is always a
/// focus to minimize this). all values that are handed to this module are true.
/// for example, this means that the consensus module doesn't know of any other
/// bundles that this guard has built except for the most profitable one. Nor
/// does it know what the proper pricing for a given storage slot is. We
/// abstract all of this out in order to keep this module as clean as possible
/// as proper functionality is critical here to ensure that Angstrom works
/// properly.
pub struct ConsensusCore {
    /// keeps track of the current round state
    round_state:        RoundState,
    /// the current overlook of the network stage
    state:              State,
    /// leader selection algo
    leader_selection:   RoundRobinAlgo,
    /// collects + formulates evidence of byzantine guards
    evidence_collector: EvidenceCollector,
    /// deals with all signing and signature verification
    signer:             Signer,
    /// messages to share with others
    outbound:           VecDeque<ConsensusMessage>
}

impl ConsensusCore {
    pub async fn new() -> Self {
        todo!()
    }

    // TODO: this should be when they officially join
    pub fn new_guard(&mut self, guard: GuardInfo) {
        self.state.next_guards.new_guard(guard);
    }

    pub fn new_block(&mut self, block: Block) {
        if self.round_state.current_height() < block.header.height {
            // TODO: wire in guard selection stuff
            self.round_state.new_height(block.header.height, false)
        }
    }

    pub fn new_proposal_vote(&mut self, vote: SignedLeaderProposal) {}

    pub fn new_proposal(&mut self, proposal: LeaderProposal) {}

    pub fn new_bundle(&mut self, bundle: SimmedBundle) {}

    pub fn new_bundle_vote(&mut self, vote: BundleVote) {}

    pub fn new_bundle_23(&mut self, bundle: Valid23Bundle) {}
}

impl Stream for ConsensusCore {
    type Item = Result<ConsensusMessage, ConsensusError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let _ = self.round_state.poll_next_unpin(cx);
        todo!()
    }
}
