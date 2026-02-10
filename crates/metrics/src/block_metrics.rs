use std::sync::OnceLock;

use prometheus::IntGaugeVec;

use crate::METRICS_ENABLED;

/// Internal metrics structure that holds all the Prometheus gauges
#[derive(Clone)]
struct BlockMetricsInner {
    // PreProposal order counts
    preproposal_limit_orders:    IntGaugeVec,
    preproposal_searcher_orders: IntGaugeVec,

    // State transition metrics
    state_slot_offset_ms:  IntGaugeVec,
    state_limit_orders:    IntGaugeVec,
    state_searcher_orders: IntGaugeVec,

    // Aggregation metrics
    preproposals_collected: IntGaugeVec,
    is_leader:              IntGaugeVec,

    // Matching engine input
    matching_input_limit:    IntGaugeVec,
    matching_input_searcher: IntGaugeVec,

    // Matching engine results
    matching_pools_solved:    IntGaugeVec,
    matching_orders_filled:   IntGaugeVec,
    matching_orders_partial:  IntGaugeVec,
    matching_orders_unfilled: IntGaugeVec,
    matching_orders_killed:   IntGaugeVec,
    bundle_generated:         IntGaugeVec,

    // Overall submission metrics (recorded from consensus layer)
    submission_started_slot_offset_ms:   IntGaugeVec,
    submission_completed_slot_offset_ms: IntGaugeVec,
    submission_latency_ms:               IntGaugeVec,
    submission_success:                  IntGaugeVec
}

impl Default for BlockMetricsInner {
    fn default() -> Self {
        let preproposal_limit_orders = prometheus::register_int_gauge_vec!(
            "ang_block_preproposal_limit_orders",
            "Limit orders when PreProposal created",
            &["block_number"]
        )
        .unwrap();

        let preproposal_searcher_orders = prometheus::register_int_gauge_vec!(
            "ang_block_preproposal_searcher_orders",
            "Searcher orders when PreProposal created",
            &["block_number"]
        )
        .unwrap();

        let state_slot_offset_ms = prometheus::register_int_gauge_vec!(
            "ang_block_state_slot_offset_ms",
            "Slot offset (ms) when state entered",
            &["block_number", "state"]
        )
        .unwrap();

        let state_limit_orders = prometheus::register_int_gauge_vec!(
            "ang_block_state_limit_orders",
            "Limit orders at state",
            &["block_number", "state"]
        )
        .unwrap();

        let state_searcher_orders = prometheus::register_int_gauge_vec!(
            "ang_block_state_searcher_orders",
            "Searcher orders at state",
            &["block_number", "state"]
        )
        .unwrap();

        let preproposals_collected = prometheus::register_int_gauge_vec!(
            "ang_block_preproposals_collected",
            "PreProposals from validators",
            &["block_number"]
        )
        .unwrap();

        let is_leader = prometheus::register_int_gauge_vec!(
            "ang_block_is_leader",
            "1 if leader, 0 otherwise",
            &["block_number"]
        )
        .unwrap();

        let matching_input_limit = prometheus::register_int_gauge_vec!(
            "ang_block_matching_input_limit",
            "Limit orders after quorum",
            &["block_number"]
        )
        .unwrap();

        let matching_input_searcher = prometheus::register_int_gauge_vec!(
            "ang_block_matching_input_searcher",
            "Searcher orders after quorum",
            &["block_number"]
        )
        .unwrap();

        let matching_pools_solved = prometheus::register_int_gauge_vec!(
            "ang_block_matching_pools_solved",
            "Pools with solutions",
            &["block_number"]
        )
        .unwrap();

        let matching_orders_filled = prometheus::register_int_gauge_vec!(
            "ang_block_matching_orders_filled",
            "Completely filled orders",
            &["block_number"]
        )
        .unwrap();

        let matching_orders_partial = prometheus::register_int_gauge_vec!(
            "ang_block_matching_orders_partial",
            "Partially filled orders",
            &["block_number"]
        )
        .unwrap();

        let matching_orders_unfilled = prometheus::register_int_gauge_vec!(
            "ang_block_matching_orders_unfilled",
            "Unfilled orders",
            &["block_number"]
        )
        .unwrap();

        let matching_orders_killed = prometheus::register_int_gauge_vec!(
            "ang_block_matching_orders_killed",
            "Killed orders",
            &["block_number"]
        )
        .unwrap();

        let bundle_generated = prometheus::register_int_gauge_vec!(
            "ang_block_bundle_generated",
            "1=bundle, 0=attestation",
            &["block_number"]
        )
        .unwrap();

        let submission_started_slot_offset_ms = prometheus::register_int_gauge_vec!(
            "ang_block_submission_started_slot_offset_ms",
            "Slot offset (ms) when submission started",
            &["block_number"]
        )
        .unwrap();

        let submission_completed_slot_offset_ms = prometheus::register_int_gauge_vec!(
            "ang_block_submission_completed_slot_offset_ms",
            "Slot offset (ms) when submission completed",
            &["block_number"]
        )
        .unwrap();

        let submission_latency_ms = prometheus::register_int_gauge_vec!(
            "ang_block_submission_latency_ms",
            "Total submission latency in milliseconds",
            &["block_number"]
        )
        .unwrap();

        let submission_success = prometheus::register_int_gauge_vec!(
            "ang_block_submission_success",
            "1=success (tx hash returned), 0=failed",
            &["block_number"]
        )
        .unwrap();

        Self {
            preproposal_limit_orders,
            preproposal_searcher_orders,
            state_slot_offset_ms,
            state_limit_orders,
            state_searcher_orders,
            preproposals_collected,
            is_leader,
            matching_input_limit,
            matching_input_searcher,
            matching_pools_solved,
            matching_orders_filled,
            matching_orders_partial,
            matching_orders_unfilled,
            matching_orders_killed,
            bundle_generated,
            submission_started_slot_offset_ms,
            submission_completed_slot_offset_ms,
            submission_latency_ms,
            submission_success
        }
    }
}

static BLOCK_METRICS_INSTANCE: OnceLock<BlockMetricsWrapper> = OnceLock::new();

/// Wrapper for block-indexed metrics. All metrics use `block_number` as a label
/// allowing Prometheus queries like `metric{block_number="12345"}` for Grafana
/// table views.
#[derive(Clone)]
pub struct BlockMetricsWrapper(Option<BlockMetricsInner>);

impl Default for BlockMetricsWrapper {
    fn default() -> Self {
        Self::new()
    }
}

impl BlockMetricsWrapper {
    /// Create or get the singleton instance
    pub fn new() -> Self {
        BLOCK_METRICS_INSTANCE
            .get_or_init(|| {
                Self(
                    METRICS_ENABLED
                        .get()
                        .copied()
                        .unwrap_or_default()
                        .then(BlockMetricsInner::default)
                )
            })
            .clone()
    }

    /// Record order counts when PreProposal is created
    pub fn record_preproposal_orders(&self, block: u64, limit: usize, searcher: usize) {
        if let Some(inner) = &self.0 {
            let block_str = block.to_string();
            inner
                .preproposal_limit_orders
                .get_metric_with_label_values(&[&block_str])
                .unwrap()
                .set(limit as i64);
            inner
                .preproposal_searcher_orders
                .get_metric_with_label_values(&[&block_str])
                .unwrap()
                .set(searcher as i64);
        }
    }

    /// Record a state transition with timing and order counts
    pub fn record_state_transition(
        &self,
        block: u64,
        state: &str,
        slot_offset_ms: u64,
        limit: usize,
        searcher: usize
    ) {
        if let Some(inner) = &self.0 {
            let block_str = block.to_string();
            let state_str = state.to_string();
            inner
                .state_slot_offset_ms
                .get_metric_with_label_values(&[&block_str, &state_str])
                .unwrap()
                .set(slot_offset_ms as i64);
            inner
                .state_limit_orders
                .get_metric_with_label_values(&[&block_str, &state_str])
                .unwrap()
                .set(limit as i64);
            inner
                .state_searcher_orders
                .get_metric_with_label_values(&[&block_str, &state_str])
                .unwrap()
                .set(searcher as i64);
        }
    }

    /// Record number of preproposals collected
    pub fn record_preproposals_collected(&self, block: u64, count: usize) {
        if let Some(inner) = &self.0 {
            inner
                .preproposals_collected
                .get_metric_with_label_values(&[&block.to_string()])
                .unwrap()
                .set(count as i64);
        }
    }

    /// Record whether this node is leader for the block
    pub fn record_is_leader(&self, block: u64, is_leader: bool) {
        if let Some(inner) = &self.0 {
            inner
                .is_leader
                .get_metric_with_label_values(&[&block.to_string()])
                .unwrap()
                .set(if is_leader { 1 } else { 0 });
        }
    }

    /// Record matching engine input order counts
    pub fn record_matching_input(&self, block: u64, limit: usize, searcher: usize) {
        if let Some(inner) = &self.0 {
            let block_str = block.to_string();
            inner
                .matching_input_limit
                .get_metric_with_label_values(&[&block_str])
                .unwrap()
                .set(limit as i64);
            inner
                .matching_input_searcher
                .get_metric_with_label_values(&[&block_str])
                .unwrap()
                .set(searcher as i64);
        }
    }

    /// Record matching engine results
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
        if let Some(inner) = &self.0 {
            let block_str = block.to_string();
            inner
                .matching_pools_solved
                .get_metric_with_label_values(&[&block_str])
                .unwrap()
                .set(pools_solved as i64);
            inner
                .matching_orders_filled
                .get_metric_with_label_values(&[&block_str])
                .unwrap()
                .set(filled as i64);
            inner
                .matching_orders_partial
                .get_metric_with_label_values(&[&block_str])
                .unwrap()
                .set(partial as i64);
            inner
                .matching_orders_unfilled
                .get_metric_with_label_values(&[&block_str])
                .unwrap()
                .set(unfilled as i64);
            inner
                .matching_orders_killed
                .get_metric_with_label_values(&[&block_str])
                .unwrap()
                .set(killed as i64);
            inner
                .bundle_generated
                .get_metric_with_label_values(&[&block_str])
                .unwrap()
                .set(if bundle_generated { 1 } else { 0 });
        }
    }

    /// Record when submission starts
    pub fn record_submission_started(&self, block: u64, slot_offset_ms: u64) {
        if let Some(inner) = &self.0 {
            inner
                .submission_started_slot_offset_ms
                .get_metric_with_label_values(&[&block.to_string()])
                .unwrap()
                .set(slot_offset_ms as i64);
        }
    }

    /// Record submission completion with results
    pub fn record_submission_completed(
        &self,
        block: u64,
        slot_offset_ms: u64,
        latency_ms: u64,
        success: bool
    ) {
        if let Some(inner) = &self.0 {
            let block_str = block.to_string();
            inner
                .submission_completed_slot_offset_ms
                .get_metric_with_label_values(&[&block_str])
                .unwrap()
                .set(slot_offset_ms as i64);
            inner
                .submission_latency_ms
                .get_metric_with_label_values(&[&block_str])
                .unwrap()
                .set(latency_ms as i64);
            inner
                .submission_success
                .get_metric_with_label_values(&[&block_str])
                .unwrap()
                .set(if success { 1 } else { 0 });
        }
    }
}
