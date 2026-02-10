use std::{
    collections::HashSet,
    task::{Context, Poll, Waker},
    time::{Duration, Instant}
};

use alloy::providers::Provider;
use angstrom_metrics::BlockMetricsWrapper;
use angstrom_types::{
    consensus::{ConsensusRoundName, PreProposalAggregation, Proposal, StromConsensusEvent},
    contract_payloads::angstrom::{AngstromBundle, BundleGasDetails},
    orders::{OrderFillState, PoolSolution},
    primitive::AngstromMetaSigner,
    sol_bindings::rpc_orders::AttestAngstromBlockEmpty,
    traits::BundleProcessing
};
use futures::{FutureExt, StreamExt, future::BoxFuture};
use matching_engine::{MatchingEngineHandle, manager::MatchingEngineError};

use super::{ConsensusState, SharedRoundState};
use crate::rounds::{ConsensusMessage, preproposal_wait_trigger::LastRoundInfo};

type MatchingEngineFuture =
    BoxFuture<'static, Result<(Vec<PoolSolution>, BundleGasDetails), MatchingEngineError>>;

/// Proposal State.
///
/// We only transition to Proposal state if we are the leader.
/// In this state we build the proposal, submit it on chain and then propagate
/// it once its landed on chain. We only submit after it has landed on chain as
/// in the case of inclusion games. the proposal will just be dropped and there
/// is no need for others to verify.
pub struct ProposalState {
    matching_engine_future: Option<MatchingEngineFuture>,
    submission_future:      Option<BoxFuture<'static, Result<bool, tokio::task::JoinError>>>,
    pre_proposal_aggs:      Vec<PreProposalAggregation>,
    proposal:               Option<Proposal>,
    last_round_info:        Option<LastRoundInfo>,
    trigger_time:           Instant,
    block_height:           u64
}

impl ProposalState {
    pub fn new<P, Matching, S: AngstromMetaSigner>(
        pre_proposal_aggregation: HashSet<PreProposalAggregation>,
        handles: &mut SharedRoundState<P, Matching, S>,
        trigger_time: Instant,
        waker: Waker
    ) -> Self
    where
        P: Provider + Unpin + 'static,
        Matching: MatchingEngineHandle
    {
        // Record state transition metrics
        let slot_offset_ms = handles.slot_offset_ms();
        let orders = handles.order_storage.get_all_orders();
        let limit_count = orders.limit.len();
        let searcher_count = orders.searcher.len();

        let metrics = BlockMetricsWrapper::new();
        metrics.record_state_transition(
            handles.block_height,
            "Proposal",
            slot_offset_ms,
            limit_count,
            searcher_count
        );

        // Count matching input orders from preproposal aggregations
        let mut matching_limit = 0usize;
        let mut matching_searcher = 0usize;
        for agg in &pre_proposal_aggregation {
            for pre in &agg.pre_proposals {
                matching_limit += pre.limit.len();
                matching_searcher += pre.searcher.len();
            }
        }
        metrics.record_matching_input(handles.block_height, matching_limit, matching_searcher);

        // queue building future
        waker.wake_by_ref();
        tracing::info!("proposal");

        Self {
            matching_engine_future: Some(
                handles.matching_engine_output(pre_proposal_aggregation.clone())
            ),
            last_round_info: None,
            pre_proposal_aggs: pre_proposal_aggregation.into_iter().collect::<Vec<_>>(),
            submission_future: None,
            proposal: None,
            trigger_time,
            block_height: handles.block_height
        }
    }

    fn try_build_proposal<P, Matching, S: AngstromMetaSigner>(
        &mut self,
        cx: &mut Context<'_>,
        result: Result<(Vec<PoolSolution>, BundleGasDetails), MatchingEngineError>,
        handles: &mut SharedRoundState<P, Matching, S>
    ) -> bool
    where
        P: Provider + Unpin + 'static,
        Matching: MatchingEngineHandle
    {
        self.last_round_info = Some(LastRoundInfo {
            time_to_complete: Instant::now().duration_since(self.trigger_time)
        });

        let provider = handles.provider.clone();
        let signer = handles.signer.clone();
        let target_block = handles.block_height + 1;

        tracing::debug!("starting to build proposal");

        let Ok(possible_bundle) = result
            .inspect_err(|e| {
                tracing::info!(err=%e,
                    "Failed to properly build proposal, THERE SHALL BE NO PROPOSAL THIS BLOCK :("
                )
            })
            .map(|(pool_solution, gas_info)| {
                // Record matching results metrics
                let metrics = BlockMetricsWrapper::new();
                let pools_solved = pool_solution.len();

                let mut filled = 0usize;
                let mut partial = 0usize;
                let mut unfilled = 0usize;
                let mut killed = 0usize;

                for solution in &pool_solution {
                    for outcome in &solution.limit {
                        match outcome.outcome {
                            OrderFillState::CompleteFill => filled += 1,
                            OrderFillState::PartialFill(_) => partial += 1,
                            OrderFillState::Unfilled => unfilled += 1,
                            OrderFillState::Killed => killed += 1
                        }
                    }
                }

                let proposal = Proposal::generate_proposal(
                    handles.block_height,
                    &handles.signer,
                    self.pre_proposal_aggs.clone(),
                    pool_solution
                );

                self.proposal = Some(proposal.clone());
                let snapshot = handles.fetch_pool_snapshot();
                let all_orders = handles.order_storage.get_all_orders();

                let bundle = AngstromBundle::from_proposal(
                    &proposal, all_orders, gas_info, &snapshot
                )
                .inspect_err(|e| {
                    tracing::info!(err=%e,
                        "failed to encode angstrom bundle, THERE SHALL BE NO PROPOSAL THIS BLOCK :("
                    );
                })
                .ok();

                // Record whether bundle was generated
                metrics.record_matching_results(
                    self.block_height,
                    pools_solved,
                    filled,
                    partial,
                    unfilled,
                    killed,
                    bundle.is_some()
                );

                bundle
            })
        else {
            return false;
        };

        let attestation = if possible_bundle.is_none() {
            AttestAngstromBlockEmpty::sign_and_encode(target_block, &signer)
        } else {
            Default::default()
        };
        handles.propagate_message(ConsensusMessage::PropagateEmptyBlockAttestation(attestation));

        let submission_future = Box::pin(async move {
            let Ok(tx_hash) = provider
                .submit_tx(signer, possible_bundle, target_block)
                .await
            else {
                tracing::error!("submission failed");
                return false;
            };

            let Some(tx_hash) = tx_hash else {
                tracing::info!("submitted unlock attestation");
                return true;
            };

            tracing::info!("submitted bundle");
            provider
                .watch_blocks()
                .await
                .unwrap()
                .with_poll_interval(Duration::from_millis(10))
                .into_stream()
                .next()
                .await;

            let included = provider
                .get_transaction_by_hash(tx_hash)
                .await
                .unwrap()
                .is_some();
            tracing::info!(?included, "block tx result");
            included
        });

        cx.waker().wake_by_ref();
        self.submission_future = Some(Box::pin(tokio::spawn(submission_future)));

        true
    }
}

impl<P, Matching, S> ConsensusState<P, Matching, S> for ProposalState
where
    P: Provider + Unpin + 'static,
    Matching: MatchingEngineHandle,
    S: AngstromMetaSigner
{
    fn on_consensus_message(
        &mut self,
        _: &mut SharedRoundState<P, Matching, S>,
        _: StromConsensusEvent
    ) {
        // No messages at this point can effect the consensus round and thus are
        // ignored.
    }

    fn poll_transition(
        &mut self,
        handles: &mut SharedRoundState<P, Matching, S>,
        cx: &mut Context<'_>
    ) -> Poll<Option<Box<dyn ConsensusState<P, Matching, S>>>> {
        if let Some(mut b_fut) = self.matching_engine_future.take() {
            match b_fut.poll_unpin(cx) {
                Poll::Ready(state) => {
                    if !self.try_build_proposal(cx, state, handles) {
                        // failed to build. we end here.
                        return Poll::Ready(None);
                    }
                }
                Poll::Pending => self.matching_engine_future = Some(b_fut)
            }
        }

        if let Some(mut b_fut) = self.submission_future.take() {
            match b_fut.poll_unpin(cx) {
                Poll::Ready(transaction_landed) => {
                    if transaction_landed.unwrap_or_default() {
                        let proposal = self.proposal.take().unwrap();
                        handles
                            .messages
                            .push_back(ConsensusMessage::PropagateProposal(proposal));
                        cx.waker().wake_by_ref();
                    }
                    return Poll::Ready(None);
                }
                Poll::Pending => self.submission_future = Some(b_fut)
            }
        }

        Poll::Pending
    }

    fn last_round_info(&mut self) -> Option<LastRoundInfo> {
        self.last_round_info.take()
    }

    fn name(&self) -> ConsensusRoundName {
        ConsensusRoundName::Proposal
    }
}
