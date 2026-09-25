use std::{
    collections::HashSet,
    task::{Context, Poll, Waker},
    time::{Duration, Instant}
};

use alloy::{primitives::B256, providers::Provider};
use angstrom_metrics::{BlockMetricsWrapper, ConsensusMetricsWrapper};
use angstrom_types::{
    consensus::{
        ConsensusRoundName, PreProposalAggregation, Proposal, SlotClock, StromConsensusEvent
    },
    contract_payloads::angstrom::AngstromBundle,
    orders::OrderFillState,
    primitive::AngstromMetaSigner,
    sol_bindings::rpc_orders::AttestAngstromBlockEmpty,
    submission::CancellationToken,
    traits::BundleProcessing
};
use futures::{FutureExt, future::BoxFuture};
use matching_engine::{MatchingEngineHandle, manager::MatchingEngineError};
use telemetry_recorder::{TelemetryMessage, telemetry_event};
use tokio::task::JoinHandle;

use super::{ConsensusState, SharedRoundState};
use crate::rounds::{ConsensusMessage, MatchingOutput, preproposal_wait_trigger::LastRoundInfo};

type MatchingEngineFuture = BoxFuture<'static, Result<MatchingOutput, MatchingEngineError>>;

/// Proposal State.
///
/// We only transition to Proposal state if we are the leader.
/// In this state we build the proposal, submit it on chain and then propagate
/// it once its landed on chain. We only submit after it has landed on chain as
/// in the case of inclusion games. the proposal will just be dropped and there
/// is no need for others to verify.
pub struct ProposalState {
    matching_engine_future: Option<MatchingEngineFuture>,
    /// The spawned submission. Aborted when this state is dropped — see the
    /// `Drop` impl — since dropping a `JoinHandle` only detaches its task.
    submission_future:      Option<JoinHandle<bool>>,
    /// Cancelled alongside the abort. The submission path re-checks it before
    /// signing and before each endpoint send, which closes the window between
    /// the abort and the task's next yield.
    cancel:                 CancellationToken,
    pre_proposal_aggs:      Vec<PreProposalAggregation>,
    proposal:               Option<Proposal>,
    last_round_info:        Option<LastRoundInfo>,
    trigger_time:           Instant,
    block_height:           u64
}

impl Drop for ProposalState {
    /// A dropped `ProposalState` is a reset round: whatever its submission has
    /// not yet sent stays unsent.
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(task) = &self.submission_future {
            task.abort();
        }
    }
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
            handles.block_height.number,
            "Proposal",
            slot_offset_ms,
            limit_count,
            searcher_count
        );

        // Count matching input orders from preproposal aggregations (pre-quorum)
        let mut matching_limit = 0usize;
        let mut matching_searcher = 0usize;
        for agg in &pre_proposal_aggregation {
            for pre in &agg.pre_proposals {
                matching_limit += pre.limit.len();
                matching_searcher += pre.searcher.len();
            }
        }
        metrics.record_matching_input_pre_quorum(
            handles.block_height.number,
            matching_limit,
            matching_searcher
        );

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
            cancel: CancellationToken::new(),
            proposal: None,
            trigger_time,
            block_height: handles.block_height.number
        }
    }

    fn try_build_proposal<P, Matching, S: AngstromMetaSigner>(
        &mut self,
        cx: &mut Context<'_>,
        result: Result<MatchingOutput, MatchingEngineError>,
        handles: &mut SharedRoundState<P, Matching, S>
    ) -> bool
    where
        P: Provider + Unpin + 'static,
        Matching: MatchingEngineHandle
    {
        let build_duration = Instant::now().duration_since(self.trigger_time);
        self.last_round_info = Some(LastRoundInfo { time_to_complete: build_duration });

        // Record proposal build time metric
        ConsensusMetricsWrapper::new()
            .set_proposal_build_time(handles.block_height.number, build_duration.as_millis());

        let provider = handles.provider.clone();
        let signer = handles.signer.clone();
        let parent = handles.block_height;
        let target_block = parent.number + 1;

        tracing::debug!("starting to build proposal");

        let output = match result {
            Ok(output) => output,
            Err(e) => {
                tracing::info!(err=%e,
                    "Failed to properly build proposal, THERE SHALL BE NO PROPOSAL THIS BLOCK :("
                );
                return false;
            }
        };

        // A result is built on only if it was produced against the parent, and in
        // the generation, this round is building for. The hash names the parent —
        // a same-height reorg changes it — and the generation catches a reset
        // that landed on the same parent.
        if output.gas.parent().hash != parent.hash || output.generation != handles.generation {
            tracing::warn!(
                built_for = ?output.gas.parent(),
                built_in_generation = output.generation,
                round = ?parent,
                round_generation = handles.generation,
                "discarding a matching result built for another round"
            );
            return false;
        }

        let MatchingOutput { solutions, gas, splits, pool_snapshots, .. } = output;

        // Record matching results metrics
        let metrics = BlockMetricsWrapper::new();
        let pools_solved = solutions.len();

        let mut filled = 0usize;
        let mut partial = 0usize;
        let mut unfilled = 0usize;
        let mut killed = 0usize;

        for solution in &solutions {
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
            parent.number,
            &handles.signer,
            self.pre_proposal_aggs.clone(),
            solutions
        );

        self.proposal = Some(proposal.clone());
        let all_orders = handles.order_storage.get_all_orders();

        // `splits` and `pool_snapshots` are the round's single reads, arriving on
        // the same result as the gas they were matched with, so this bundle and
        // the one the gas was estimated for are built from the same rates and
        // the same pool state.
        let possible_bundle = AngstromBundle::from_proposal(
            &proposal,
            all_orders,
            gas,
            &pool_snapshots,
            splits.splits
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
            possible_bundle.is_some()
        );

        let attestation = if possible_bundle.is_none() {
            AttestAngstromBlockEmpty::sign_and_encode(target_block, &signer)
        } else {
            Default::default()
        };
        handles.propagate_message(ConsensusMessage::PropagateEmptyBlockAttestation(attestation));

        // Capture slot clock for metrics timing
        let slot_clock = handles.slot_clock.clone();
        let block_height = parent.number;
        let cancel = self.cancel.clone();

        // Every bundle handed to submission is attributable to the parent it was
        // priced on; the eth manager records the parent it lands on. Recorded
        // before the task starts, so a reset that aborts it mid-send cannot lose
        // the one datum that is unrecoverable afterwards. A record for a bundle
        // that never left the node pairs with nothing and is harmless.
        if let Some(bundle) = &possible_bundle {
            let order_hashes: Vec<B256> = bundle.get_order_hashes(target_block).collect();
            telemetry_event!(TelemetryMessage::bundle_submitted(
                target_block,
                parent,
                order_hashes
            ));
        }

        let submission_future = async move {
            // Record submission start
            let slot_duration = slot_clock.slot_duration();
            let next_slot = slot_clock.duration_to_next_slot().unwrap_or(slot_duration);
            let start_offset_ms = slot_duration.saturating_sub(next_slot).as_millis() as u64;
            let start_time = std::time::Instant::now();

            let metrics = BlockMetricsWrapper::new();
            metrics.record_submission_started(block_height, start_offset_ms);

            let result = provider
                .submit_tx(signer, possible_bundle, parent, cancel)
                .await;

            let latency_ms = start_time.elapsed().as_millis() as u64;
            let end_offset_ms = start_offset_ms + latency_ms;

            match &result {
                Ok(all_results) => {
                    // Record metrics for EACH endpoint attempt
                    for submission_result in all_results {
                        metrics.record_submission_endpoint(
                            block_height,
                            &submission_result.submitter_type,
                            &submission_result.endpoint,
                            submission_result.success,
                            submission_result.latency_ms
                        );
                    }

                    // Check if any submission succeeded
                    let any_success = all_results.iter().any(|r| r.success);
                    metrics.record_submission_completed(
                        block_height,
                        end_offset_ms,
                        latency_ms,
                        any_success
                    );
                }
                Err(_) => {
                    metrics.record_submission_completed(
                        block_height,
                        end_offset_ms,
                        latency_ms,
                        false
                    );
                }
            }

            let all_results = match result {
                Ok(all_results) => all_results,
                Err(e) => {
                    tracing::error!(err=%e, "submission failed");
                    return false;
                }
            };

            let successful_tx_hashes: HashSet<_> = all_results
                .iter()
                .filter(|result| result.success)
                .filter_map(|result| result.tx_hash)
                .collect();

            if successful_tx_hashes.is_empty() {
                // Check if any succeeded (attestation-only case)
                if all_results.iter().any(|r| r.success) {
                    tracing::info!("submitted unlock attestation");
                    return true;
                }
                tracing::error!("no successful submissions");
                return false;
            }

            tracing::info!(
                candidate_submission_tx_hashes = successful_tx_hashes.len(),
                "submitted bundle"
            );

            // Wait for the target block to be produced
            // We poll until the block exists rather than using watch_blocks()
            // which can return stale block hashes from its filter buffer
            let target_block_body = loop {
                match provider.get_block_by_number(target_block.into()).await {
                    Ok(Some(block)) => break block,
                    Ok(None) => {
                        tokio::time::sleep(Duration::from_millis(250)).await;
                    }
                    Err(e) => {
                        tracing::warn!(?e, "error polling for target block");
                        tokio::time::sleep(Duration::from_millis(250)).await;
                    }
                }
            };

            let included_tx_hash = target_block_body
                .transactions
                .hashes()
                .find(|block_tx_hash| successful_tx_hashes.contains(block_tx_hash));

            let included = included_tx_hash.is_some();

            // Record bundle inclusion metric
            metrics.record_bundle_included(block_height, included);

            tracing::info!(
                ?included,
                target_block,
                candidate_submission_tx_hashes = successful_tx_hashes.len(),
                ?included_tx_hash,
                "block tx result"
            );
            included
        };

        cx.waker().wake_by_ref();
        self.submission_future = Some(tokio::spawn(submission_future));

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

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        pin::Pin,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering}
        },
        task::Context,
        time::{Duration, Instant}
    };

    use alloy::{
        eips::BlockNumHash,
        primitives::{Address, B256, U64},
        rpc::types::FeeHistory,
        signers::local::PrivateKeySigner,
        transports::mock::Asserter
    };
    use angstrom_types::{
        contract_payloads::angstrom::{AngstromBundle, BundleGasDetails},
        primitive::{AngstromAddressConfig, UniswapPoolRegistry},
        submission::{ChainSubmitterWrapper, SubmissionResult, TxFeatureInfo}
    };
    use testing_tools::mocks::matching_engine::MockMatchingEngine;
    use tokio::sync::Notify;

    use super::ProposalState;
    use crate::rounds::{
        ConsensusMessage, ConsensusState, MatchingOutput, RoundStateMachine,
        tests::{ProviderDef, setup_state_machine_with}
    };

    type Machine = RoundStateMachine<ProviderDef, MockMatchingEngine, PrivateKeySigner>;

    /// Counts submissions instead of sending them. `gate` holds a submission in
    /// flight so a reset can land while it is. It is deliberately blind to the
    /// cancellation token: only the abort can keep its count at zero.
    #[derive(Clone, Default)]
    struct CountingSubmitter {
        entered: Arc<Notify>,
        gate:    Arc<Notify>,
        sends:   Arc<AtomicUsize>
    }

    impl ChainSubmitterWrapper for CountingSubmitter {
        fn angstrom_address(&self) -> Address {
            Address::ZERO
        }

        fn submit<'a>(
            &'a self,
            _: Option<&'a AngstromBundle>,
            _: &'a TxFeatureInfo
        ) -> Pin<Box<dyn Future<Output = eyre::Result<Vec<SubmissionResult>>> + Send + 'a>>
        {
            Box::pin(async move {
                self.entered.notify_one();
                self.gate.notified().await;
                self.sends.fetch_add(1, Ordering::SeqCst);
                Ok(vec![SubmissionResult {
                    tx_hash:        None,
                    submitter_type: "counting".into(),
                    endpoint:       "counting".into(),
                    success:        true,
                    latency_ms:     0
                }])
            })
        }
    }

    /// A machine whose node answers `submit_tx`'s preparation — nonce, fee
    /// history, chain id, in that order — and submits through `submitter`.
    async fn machine_with(submitter: CountingSubmitter) -> Machine {
        AngstromAddressConfig::INTERNAL_TESTNET.try_init();
        let asserter = Asserter::new();
        asserter.push_success(&U64::ZERO);
        asserter.push_success(&FeeHistory {
            base_fee_per_gas: vec![1, 1],
            reward: Some(vec![vec![1]]),
            ..Default::default()
        });
        asserter.push_success(&U64::from(1));
        setup_state_machine_with(
            asserter,
            vec![Box::new(submitter)],
            UniswapPoolRegistry::default()
        )
        .await
    }

    fn cx() -> Context<'static> {
        Context::from_waker(futures::task::noop_waker_ref())
    }

    /// A proposal state ready to be handed a matching result directly. The
    /// future `new` queued is dropped so nothing else drives a build.
    fn proposal_state(machine: &mut Machine) -> ProposalState {
        let mut state = ProposalState::new(
            HashSet::default(),
            &mut machine.shared_state,
            Instant::now(),
            futures::task::noop_waker_ref().to_owned()
        );
        state.matching_engine_future = None;
        state
    }

    async fn current_output(machine: &Machine) -> MatchingOutput {
        machine
            .shared_state
            .matching_engine_output(HashSet::default())
            .await
            .unwrap()
    }

    fn assert_nothing_built(state: &ProposalState, machine: &Machine) {
        assert!(state.proposal.is_none(), "a proposal was built on a stale result");
        assert!(state.submission_future.is_none(), "a submission was started for a stale result");
        assert!(machine.shared_state.messages.is_empty(), "something was propagated for it");
    }

    /// The head moves while the round is in flight — to another height, and to
    /// another hash at the same height — and the result matched on the old
    /// parent is not built on.
    #[tokio::test]
    async fn a_result_for_a_parent_the_head_moved_from_is_not_built_on() {
        for moved_to in
            [BlockNumHash::new(2, B256::repeat_byte(2)), BlockNumHash::new(1, B256::repeat_byte(9))]
        {
            let mut machine = machine_with(CountingSubmitter::default()).await;
            let stale = current_output(&machine).await;
            let leader = machine.shared_state.round_leader;
            machine.reset_round(moved_to, leader);

            let mut state = proposal_state(&mut machine);
            assert!(!state.try_build_proposal(&mut cx(), Ok(stale), &mut machine.shared_state));
            assert_nothing_built(&state, &machine);
        }
    }

    /// The parent check on its own: a result stamped with another parent in
    /// the current generation is discarded.
    #[tokio::test]
    async fn a_result_stamped_with_another_parent_is_not_built_on() {
        let mut machine = machine_with(CountingSubmitter::default()).await;
        let mut foreign = current_output(&machine).await;
        foreign.gas = BundleGasDetails::new(0, BlockNumHash::new(1, B256::repeat_byte(9)));

        let mut state = proposal_state(&mut machine);
        assert!(!state.try_build_proposal(&mut cx(), Ok(foreign), &mut machine.shared_state));
        assert_nothing_built(&state, &machine);
    }

    /// The generation check on its own: a reset that lands on the very same
    /// parent still invalidates the result captured before it.
    #[tokio::test]
    async fn a_result_from_before_a_reset_on_the_same_parent_is_not_built_on() {
        let mut machine = machine_with(CountingSubmitter::default()).await;
        let before_reset = current_output(&machine).await;
        let (parent, leader) =
            (machine.shared_state.block_height, machine.shared_state.round_leader);
        machine.reset_round(parent, leader);

        let mut state = proposal_state(&mut machine);
        assert!(!state.try_build_proposal(&mut cx(), Ok(before_reset), &mut machine.shared_state));
        assert_nothing_built(&state, &machine);
    }

    /// A reset lands while the submission is in flight, parked at the
    /// submitter: nothing is sent afterwards. The submitter counts what a
    /// merely detached task would have sent.
    #[tokio::test]
    async fn a_reset_round_makes_no_further_sends() {
        let submitter = CountingSubmitter::default();
        let mut machine = machine_with(submitter.clone()).await;
        let output = current_output(&machine).await;

        let mut state = proposal_state(&mut machine);
        assert!(state.try_build_proposal(&mut cx(), Ok(output), &mut machine.shared_state));
        machine.set_state_machine_at(Box::new(state));
        submitter.entered.notified().await;

        let leader = machine.shared_state.round_leader;
        machine.reset_round(BlockNumHash::new(2, B256::repeat_byte(2)), leader);
        submitter.gate.notify_one();
        tokio::time::sleep(Duration::from_millis(100)).await;

        assert_eq!(submitter.sends.load(Ordering::SeqCst), 0, "sent after the round was reset");
    }

    /// The control: a round that is not reset submits exactly once, and the
    /// round ends on the outcome.
    #[tokio::test]
    async fn a_round_that_is_not_reset_still_submits() {
        let submitter = CountingSubmitter::default();
        let mut machine = machine_with(submitter.clone()).await;
        let output = current_output(&machine).await;

        let mut state = proposal_state(&mut machine);
        assert!(state.try_build_proposal(&mut cx(), Ok(output), &mut machine.shared_state));
        assert!(matches!(
            machine.shared_state.messages.pop_front(),
            Some(ConsensusMessage::PropagateEmptyBlockAttestation(_))
        ));
        submitter.entered.notified().await;
        submitter.gate.notify_one();

        let next = futures::future::poll_fn(|cx| {
            ConsensusState::poll_transition(&mut state, &mut machine.shared_state, cx)
        })
        .await;
        assert!(next.is_none(), "the round ends once the submission resolves");
        assert_eq!(submitter.sends.load(Ordering::SeqCst), 1);
    }
}
