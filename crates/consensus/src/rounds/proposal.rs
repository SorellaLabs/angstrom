use std::{
    collections::HashSet,
    task::{Context, Poll, Waker},
    time::Duration
};

use alloy::{network::TransactionBuilder, providers::Provider, rpc::types::TransactionRequest};
use angstrom_network::manager::StromConsensusEvent;
use angstrom_types::{
    consensus::{PreProposalAggregation, Proposal},
    contract_payloads::angstrom::{AngstromBundle, BundleGasDetails},
    orders::PoolSolution
};
use futures::{future::BoxFuture, FutureExt, StreamExt};
use matching_engine::MatchingEngineHandle;
use pade::PadeEncode;

use super::{ConsensusState, SharedRoundState};
use crate::rounds::ConsensusMessage;

type MatchingEngineFuture = BoxFuture<'static, eyre::Result<(Vec<PoolSolution>, BundleGasDetails)>>;

/// Proposal State.
///
/// We only transition to Proposal state if we are the leader.
/// In this state we build the proposal, submit it on chain and then propagate
/// it once its landed on chain. We only submit after it has landed on chain as
/// in the case of inclusion games. the proposal will just be dropped and there
/// is no need for others to verify.
pub struct ProposalState {
    matching_engine_future: Option<MatchingEngineFuture>,
    submission_future:      Option<BoxFuture<'static, bool>>,
    pre_proposal_aggs:      Vec<PreProposalAggregation>,
    proposal:               Option<Proposal>,
    waker:                  Waker
}

impl ProposalState {
    pub fn new<P, Matching>(
        pre_proposal_aggregation: HashSet<PreProposalAggregation>,
        handles: &mut SharedRoundState<P, Matching>,
        waker: Waker
    ) -> Self
    where
        P: Provider + 'static,
        Matching: MatchingEngineHandle
    {
        // queue building future
        waker.wake_by_ref();

        Self {
            matching_engine_future: Some(
                handles.matching_engine_output(pre_proposal_aggregation.clone())
            ),
            pre_proposal_aggs: pre_proposal_aggregation.into_iter().collect::<Vec<_>>(),
            submission_future: None,
            proposal: None,
            waker
        }
    }

    fn try_build_proposal<P, Matching>(
        &mut self,
        result: eyre::Result<(Vec<PoolSolution>, BundleGasDetails)>,
        handles: &mut SharedRoundState<P, Matching>
    ) -> bool
    where
        P: Provider + 'static,
        Matching: MatchingEngineHandle
    {
        let Ok((pool_solution, gas_info)) = result else {
            tracing::error!(
                "Failed to properly build proposal, THERE SHALL BE NO PROPOSAL THIS BLOCK :("
            );
            return false
        };

        let proposal = Proposal::generate_proposal(
            handles.block_height,
            &handles.signer,
            self.pre_proposal_aggs.clone(),
            pool_solution
        );

        self.proposal = Some(proposal.clone());
        let snapshot = handles.fetch_pool_snapshot();

        let Ok(bundle) = AngstromBundle::from_proposal(&proposal, gas_info, &snapshot) else {
            tracing::error!(
                "failed to encode angstrom bundle, THERE SHALL BE NO PROPOSAL THIS BLOCK :("
            );
            return false
        };

        let mut tx = TransactionRequest::default()
            .with_to(handles.angstrom_address)
            .with_from(handles.signer.address())
            .with_input(bundle.pade_encode());

        let provider = handles.provider.clone();
        let signer = handles.signer.clone();

        let submission_future = async move {
            provider
                .populate_gas_nonce_chain_id(signer.address(), &mut tx)
                .await;

            let (hash, success) = provider.sign_and_send(signer, tx).await;
            if !success {
                return false
            }

            // wait for next block. then see if transaction landed
            provider
                .watch_blocks()
                .await
                .unwrap()
                .with_poll_interval(Duration::from_millis(10))
                .into_stream()
                .next()
                .await;

            provider
                .get_transaction_by_hash(hash)
                .await
                .unwrap()
                .is_some()
        }
        .boxed();

        self.waker.wake_by_ref();
        self.submission_future = Some(submission_future);

        true
    }
}

impl<P, Matching> ConsensusState<P, Matching> for ProposalState
where
    P: Provider + 'static,
    Matching: MatchingEngineHandle
{
    fn on_consensus_message(
        &mut self,
        _: &mut SharedRoundState<P, Matching>,
        _: StromConsensusEvent
    ) {
        // No messages at this point can effect the consensus round and thus are
        // ignored.
    }

    fn poll_transition(
        &mut self,
        handles: &mut SharedRoundState<P, Matching>,
        cx: &mut Context<'_>
    ) -> Poll<Option<Box<dyn ConsensusState<P, Matching>>>> {
        if let Some(mut b_fut) = self.matching_engine_future.take() {
            match b_fut.poll_unpin(cx) {
                Poll::Ready(state) => {
                    if !self.try_build_proposal(state, handles) {
                        // failed to build. we end here.
                        return Poll::Ready(None)
                    }
                }
                Poll::Pending => self.matching_engine_future = Some(b_fut)
            }
        }

        if let Some(mut b_fut) = self.submission_future.take() {
            match b_fut.poll_unpin(cx) {
                Poll::Ready(transaction_landed) => {
                    if transaction_landed {
                        let proposal = self.proposal.take().unwrap();
                        handles
                            .messages
                            .push_back(ConsensusMessage::PropagateProposal(proposal));
                        cx.waker().wake_by_ref();
                    }
                    return Poll::Ready(None)
                }
                Poll::Pending => self.submission_future = Some(b_fut)
            }
        }

        Poll::Pending
    }
}
