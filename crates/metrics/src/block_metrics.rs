use std::sync::OnceLock;

use crate::{
    METRICS_ENABLED,
    block_metrics_db::{BlockMetricEvent, enqueue_block_metric_event}
};

#[derive(Clone)]
struct BlockMetrics;

impl BlockMetrics {
    fn record_preproposal_orders(&self, block: u64, limit: usize, searcher: usize) {
        enqueue_block_metric_event(BlockMetricEvent::PreproposalOrders { block, limit, searcher });
    }

    fn record_state_transition(
        &self,
        block: u64,
        state: &str,
        slot_offset_ms: u64,
        limit: usize,
        searcher: usize
    ) {
        enqueue_block_metric_event(BlockMetricEvent::StateTransition {
            block,
            state: state.to_string(),
            slot_offset_ms,
            limit,
            searcher
        });
    }

    fn record_preproposals_collected(&self, block: u64, count: usize) {
        enqueue_block_metric_event(BlockMetricEvent::PreproposalsCollected { block, count });
    }

    fn record_is_leader(&self, block: u64, is_leader: bool) {
        enqueue_block_metric_event(BlockMetricEvent::IsLeader { block, is_leader });
    }

    fn record_matching_input_pre_quorum(&self, block: u64, limit: usize, searcher: usize) {
        enqueue_block_metric_event(BlockMetricEvent::MatchingInputPreQuorum {
            block,
            limit,
            searcher
        });
    }

    fn record_matching_input_post_quorum(&self, block: u64, limit: usize, searcher: usize) {
        enqueue_block_metric_event(BlockMetricEvent::MatchingInputPostQuorum {
            block,
            limit,
            searcher
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn record_matching_results(
        &self,
        block: u64,
        pools_solved: usize,
        filled: usize,
        partial: usize,
        unfilled: usize,
        killed: usize,
        bundle_generated: bool
    ) {
        enqueue_block_metric_event(BlockMetricEvent::MatchingResults {
            block,
            pools_solved,
            filled,
            partial,
            unfilled,
            killed,
            bundle_generated
        });
    }

    fn record_submission_started(&self, block: u64, slot_offset_ms: u64) {
        enqueue_block_metric_event(BlockMetricEvent::SubmissionStarted { block, slot_offset_ms });
    }

    fn record_submission_completed(
        &self,
        block: u64,
        slot_offset_ms: u64,
        latency_ms: u64,
        success: bool
    ) {
        enqueue_block_metric_event(BlockMetricEvent::SubmissionCompleted {
            block,
            slot_offset_ms,
            latency_ms,
            success
        });
    }

    fn record_submission_endpoint(
        &self,
        block: u64,
        submitter_type: &str,
        endpoint: &str,
        success: bool,
        latency_ms: u64
    ) {
        enqueue_block_metric_event(BlockMetricEvent::SubmissionEndpoint {
            block,
            submitter_type: submitter_type.to_string(),
            endpoint: endpoint.to_string(),
            success,
            latency_ms
        });
    }

    fn record_bundle_included(&self, block: u64, included: bool) {
        enqueue_block_metric_event(BlockMetricEvent::BundleIncluded { block, included });
    }
}

static BLOCK_METRICS_INSTANCE: OnceLock<BlockMetricsWrapper> = OnceLock::new();

#[derive(Clone)]
pub struct BlockMetricsWrapper(Option<BlockMetrics>);

impl Default for BlockMetricsWrapper {
    fn default() -> Self {
        Self::new()
    }
}

impl BlockMetricsWrapper {
    pub fn new() -> Self {
        BLOCK_METRICS_INSTANCE
            .get_or_init(|| {
                Self(
                    METRICS_ENABLED
                        .get()
                        .copied()
                        .unwrap_or_default()
                        .then_some(BlockMetrics)
                )
            })
            .clone()
    }

    pub fn record_preproposal_orders(&self, block: u64, limit: usize, searcher: usize) {
        if let Some(this) = self.0.as_ref() {
            this.record_preproposal_orders(block, limit, searcher);
        }
    }

    pub fn record_state_transition(
        &self,
        block: u64,
        state: &str,
        slot_offset_ms: u64,
        limit: usize,
        searcher: usize
    ) {
        if let Some(this) = self.0.as_ref() {
            this.record_state_transition(block, state, slot_offset_ms, limit, searcher);
        }
    }

    pub fn record_preproposals_collected(&self, block: u64, count: usize) {
        if let Some(this) = self.0.as_ref() {
            this.record_preproposals_collected(block, count);
        }
    }

    pub fn record_is_leader(&self, block: u64, is_leader: bool) {
        if let Some(this) = self.0.as_ref() {
            this.record_is_leader(block, is_leader);
        }
    }

    pub fn record_matching_input_pre_quorum(&self, block: u64, limit: usize, searcher: usize) {
        if let Some(this) = self.0.as_ref() {
            this.record_matching_input_pre_quorum(block, limit, searcher);
        }
    }

    pub fn record_matching_input_post_quorum(&self, block: u64, limit: usize, searcher: usize) {
        if let Some(this) = self.0.as_ref() {
            this.record_matching_input_post_quorum(block, limit, searcher);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_matching_results(
        &self,
        block: u64,
        pools_solved: usize,
        filled: usize,
        partial: usize,
        unfilled: usize,
        killed: usize,
        bundle_generated: bool
    ) {
        if let Some(this) = self.0.as_ref() {
            this.record_matching_results(
                block,
                pools_solved,
                filled,
                partial,
                unfilled,
                killed,
                bundle_generated
            );
        }
    }

    pub fn record_submission_started(&self, block: u64, slot_offset_ms: u64) {
        if let Some(this) = self.0.as_ref() {
            this.record_submission_started(block, slot_offset_ms);
        }
    }

    pub fn record_submission_completed(
        &self,
        block: u64,
        slot_offset_ms: u64,
        latency_ms: u64,
        success: bool
    ) {
        if let Some(this) = self.0.as_ref() {
            this.record_submission_completed(block, slot_offset_ms, latency_ms, success);
        }
    }

    pub fn record_submission_endpoint(
        &self,
        block: u64,
        submitter_type: &str,
        endpoint: &str,
        success: bool,
        latency_ms: u64
    ) {
        if let Some(this) = self.0.as_ref() {
            this.record_submission_endpoint(block, submitter_type, endpoint, success, latency_ms);
        }
    }

    pub fn record_bundle_included(&self, block: u64, included: bool) {
        if let Some(this) = self.0.as_ref() {
            this.record_bundle_included(block, included);
        }
    }
}
