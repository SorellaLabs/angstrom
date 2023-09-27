use std::{
    borrow::Cow,
    collections::VecDeque,
    pin::Pin,
    task::{Context, Poll, Waker}
};

use futures::{stream::FuturesUnordered, Stream, StreamExt};
// use guard_provider::ProviderFactory;
use guard_types::{
    consensus::{
        Block, BundleVote, EvidenceError, GuardSet, LeaderProposal, SignedLeaderProposal,
        Valid23Bundle
    },
    database::{BlockId, State},
    on_chain::SimmedBundle
};
use reth_db::{mdbx::Env, DatabaseEnv};
use sim::Simulator;
use thiserror::Error;
use tokio::task::{JoinError, JoinHandle, JoinSet};
use tracing::{error, warn};

use crate::{
    evidence::EvidenceCollector,
    executor::{Executor, ExecutorMessage},
    round::RoundState,
    state::ChainMaintainer
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
/// This includes but not limited to.
///
/// 1) Collecting Evidence against misbehavior
/// 2) Verifying Block and bundle data
/// 3) Leader Selection
/// 4) Verifying Historical State
/// 5) Signing Votes, Commitments & Proposals
pub struct ConsensusCore<S: Simulator + 'static> {
    evidence_collector: EvidenceCollector,
    round_state:        RoundState,
    state:              State,

    executor:         Executor<S>,
    /// u8 is a placeholder till we unblackbox db
    chain_maintainer: ChainMaintainer<u8>,

    outbound: VecDeque<ConsensusMessage>
}

impl<S: Simulator + 'static> ConsensusCore<S> {
    pub async fn new() -> Self {
        todo!()
    }

    pub fn new_proposal_vote(&mut self, vote: SignedLeaderProposal) {
        if !self.round_state.stage().is_past_proposal_vote_cutoff()
            && self
                .round_state
                .proposal_manager()
                .new_proposal_vote(vote.clone(), &self.state.guards)
        {
            self.outbound
                .push_back(ConsensusMessage::SignedProposal(vote))
        }
    }

    pub fn new_proposal(&mut self, proposal: LeaderProposal) {
        if self.round_state.stage().is_past_proposal_cutoff() {
            warn!(?proposal, "received proposal to late");
            return
        }

        if self.round_state.proposal_manager().has_proposal() {
            return
        }

        let Some(current_leader) = self.state.guards.get_current_leader() else { return };

        // validate signatures on proposal
        if !proposal.validate_signature(current_leader.pub_key) {
            warn!(?proposal, "failed to validate signatures");
            return
        }

        let Ok(proposal_vote) = self.executor.sign_leader_proposal(&proposal) else {
            error!("failed to sign the leader proposal");
            return
        };

        // verify the bundle is the best
        if !self.round_state.bundle().is_best_bundle(&proposal.bundle) {
            error!(?proposal, "the proposed bundle doesn't match our best bundle");
        }

        self.round_state.proposal_manager().new_proposal(proposal.clone());
        self.round_state.proposal_manager()
            .new_proposal_vote(proposal_vote.clone(), &self.state.guards);

        self.outbound.extend(
            vec![
                ConsensusMessage::Proposal(proposal),
                ConsensusMessage::SignedProposal(proposal_vote),
            ]
            .into_iter()
        );
    }

    pub fn new_bundle(&mut self, bundle: SimmedBundle) {
        if self.round_state.stage().is_past_bundle_signing_cutoff() {
            return
        }

        if let Some(hash) = self.round_state.bundle().new_simmed_bundle(bundle) {
            // new bundle, lets sign and propagate our hash
            let Ok(signed_bundle) =
                self.executor
                    .sign_bundle_vote(hash, self.round_state.stage().height(), self.round_state.stage().round())
            else {
                return
            };

            self.outbound
                .push_back(ConsensusMessage::NewBundleVote(signed_bundle.clone()));

            // add vote to underlying and if we hit 2/3 we fully propagate
            if let Some(msg) = self.
                round_state
                .bundle()
                .new_bundle_vote(signed_bundle, &self.state.guards)
            {
                self.outbound.push_back(ConsensusMessage::NewBundle23(msg));
            }
        }
    }

    pub fn new_bundle_vote(&mut self, vote: BundleVote) {
        if self.round_state.stage().is_past_bundle_signing_cutoff() {
            return
        }

        if !self.round_state.bundle().contains_vote(&vote) {
            if let Some(valid23) = self.round_state.bundle().new_bundle_vote(vote.clone(), &self.state.guards) {
                self.outbound
                    .push_back(ConsensusMessage::NewBundle23(valid23));
            }
            self.outbound
                .push_back(ConsensusMessage::NewBundleVote(vote));
        }
    }

    pub fn new_bundle_23(&mut self, bundle: Valid23Bundle) {
        if self.round_state.stage().is_past_bundle23_prop_cutoff() {
            return
        }

        if self.round_state.bundle().new_bundle23(bundle.clone(), &self.state.guards) {
            self.outbound
                .push_back(ConsensusMessage::NewBundle23(bundle.clone()))
        }
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
        self.round_state.stage().update_current_stage();

        let stuff = self.executor.poll(cx);

        todo!()
    }
}
