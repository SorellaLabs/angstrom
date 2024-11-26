use std::time::Instant;

use prometheus::{Histogram, HistogramVec, IntGauge};

use crate::METRICS_ENABLED;

#[derive(Clone)]
struct ConsensusMetricsInner {
    /// current block height
    block_height:              IntGauge,
    /// time (ms) it takes a round of consensus to complete per block
    completion_time_per_block: Histogram,
    /// The amount of time spent in each stage of the state machine (us)
    round_duration:            HistogramVec,
    /// the time us it takes to select the new leader
    round_robin_duration:      Histogram
}

impl Default for ConsensusMetricsInner {
    fn default() -> Self {
        let block_height =
            prometheus::register_int_gauge!("consensus_block_height", "current block height",)
                .unwrap();

        let buckets = prometheus::exponential_buckets(1.0, 2.0, 15).unwrap();

        let completion_time_per_block = prometheus::register_histogram!(
            "consensus_completion_time_per_block",
            "time for a round of consensus to complete ms",
            buckets.clone()
        )
        .unwrap();

        let round_robin_duration = prometheus::register_histogram!(
            "consensus_round_robin_algo_time",
            "time it takes in us for the round robin algo to calculate a new leader",
            buckets.clone()
        )
        .unwrap();

        let round_duration = prometheus::register_histogram_vec!(
            "consensus_round_duration",
            "time for a different state machine steps consensus to complete us",
            &["round_name"],
            buckets.clone()
        )
        .unwrap();

        Self { block_height, completion_time_per_block, round_duration, round_robin_duration }
    }
}

impl ConsensusMetricsInner {
    fn set_block_height(&self, block_number: u64) {
        self.block_height.set(block_number as i64);
    }

    fn measure_lifetime();
}

#[derive(Clone)]
pub struct ConsensusMetrics(Option<ConsensusMetricsInner>);

impl Default for ConsensusMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsensusMetrics {
    pub fn new() -> Self {
        Self(
            METRICS_ENABLED
                .get()
                .copied()
                .unwrap_or_default()
                .then(ConsensusMetricsInner::default)
        )
    }

    pub fn set_block_height(&self, block_number: u64) {
        if let Some(this) = self.0.as_ref() {
            this.set_block_height(block_number)
        }
    }
}
