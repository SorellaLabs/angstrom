use std::{collections::HashMap, time::Instant};

use prometheus::{IntGauge, IntGaugeVec};

use crate::METRICS_ENABLED;

#[derive(Clone)]
struct ConsensusMetrics {
    // current block height
    block_height: IntGauge,
    // time (ms) it takes a round of consensus to complete per block
    completion_time_per_block: IntGaugeVec,
    // time (ms) it takes to build a proposal per block
    proposal_build_time_per_block: IntGaugeVec,
    // time (ms) it takes proposal verification per block
    proposal_verification_time_per_block: IntGaugeVec,
    // map of block numbers to their consensus start times
    block_consensus_start_times: HashMap<u64, Instant>
}

impl Default for ConsensusMetrics {
    fn default() -> Self {
        let block_height =
            prometheus::register_int_gauge!("consensus_block_height", "current block height",)
                .unwrap();

        let proposal_build_time_per_block = prometheus::register_int_gauge_vec!(
            "consensus_proposal_build_time_per_block",
            "time (ms) it takes to build a proposal per block",
            &["block_number"]
        )
        .unwrap();

        let proposal_verification_time_per_block = prometheus::register_int_gauge_vec!(
            "consensus_proposal_verification_time_per_block",
            "time (ms) it takes proposal verification per block",
            &["block_number"]
        )
        .unwrap();

        let completion_time_per_block = prometheus::register_int_gauge_vec!(
            "consensus_completion_time_per_block",
            "time (ms) it takes a round of consensus to complete per block",
            &["block_number"]
        )
        .unwrap();

        Self {
            block_height,
            proposal_build_time_per_block,
            completion_time_per_block,
            proposal_verification_time_per_block,
            block_consensus_start_times: HashMap::default()
        }
    }
}

impl ConsensusMetrics {
    pub fn set_consensus_completion_time(&self, block_number: u64, time: u128) {
        self.completion_time_per_block
            .get_metric_with_label_values(&[&block_number.to_string()])
            .unwrap()
            .set(time as i64);
    }

    pub fn set_proposal_verification_time(&self, block_number: u64, time: u128) {
        self.proposal_verification_time_per_block
            .get_metric_with_label_values(&[&block_number.to_string()])
            .unwrap()
            .set(time as i64);
    }

    pub fn set_proposal_build_time(&self, block_number: u64, time: u128) {
        self.proposal_build_time_per_block
            .get_metric_with_label_values(&[&block_number.to_string()])
            .unwrap()
            .set(time as i64);
    }

    pub fn set_block_height(&mut self, block_number: u64) {
        self.block_height.set(block_number as i64);
        self.block_consensus_start_times
            .insert(block_number, Instant::now());
    }

    pub fn set_commit_time(&mut self, block_number: u64) {
        let time = self
            .block_consensus_start_times
            .remove(&block_number)
            .unwrap()
            .elapsed()
            .as_millis();
        self.completion_time_per_block
            .get_metric_with_label_values(&[&block_number.to_string()])
            .unwrap()
            .set(time as i64);
    }
}

#[derive(Clone)]
pub struct ConsensusMetricsWrapper(Option<ConsensusMetrics>);

impl Default for ConsensusMetricsWrapper {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsensusMetricsWrapper {
    pub fn new() -> Self {
        Self(
            METRICS_ENABLED
                .get()
                .copied()
                .unwrap_or_default()
                .then(ConsensusMetrics::default)
        )
    }

    pub fn set_consensus_completion_time(&self, block_number: u64, time: u128) {
        if let Some(this) = self.0.as_ref() {
            this.set_consensus_completion_time(block_number, time)
        }
    }

    pub fn set_proposal_verification_time(&self, block_number: u64, time: u128) {
        if let Some(this) = self.0.as_ref() {
            this.set_proposal_verification_time(block_number, time)
        }
    }

    pub fn set_proposal_build_time(&self, block_number: u64, time: u128) {
        if let Some(this) = self.0.as_ref() {
            this.set_proposal_build_time(block_number, time)
        }
    }

    pub fn set_block_height(&mut self, block_number: u64) {
        if let Some(this) = self.0.as_mut() {
            this.set_block_height(block_number)
        }
    }

    pub fn set_commit_time(&mut self, block_number: u64) {
        if let Some(this) = self.0.as_mut() {
            this.set_commit_time(block_number)
        }
    }
}
