use std::sync::OnceLock;

use angstrom_rpc_types::metrics::BlockMetricsEvent;
use prometheus::IntGauge;

use crate::{METRICS_ENABLED, block_metrics_stream::publish_block_metrics_event};

#[derive(Clone)]
struct ConsensusMetrics {
    block_height: IntGauge
}

impl Default for ConsensusMetrics {
    fn default() -> Self {
        let block_height =
            prometheus::register_int_gauge!("consensus_block_height", "current block height")
                .unwrap();
        Self { block_height }
    }
}

impl ConsensusMetrics {
    fn set_block_height(&self, block_number: u64) {
        self.block_height.set(block_number as i64);
    }
}

static METRICS_INSTANCE: OnceLock<ConsensusMetricsWrapper> = OnceLock::new();

#[derive(Clone)]
pub struct ConsensusMetricsWrapper(Option<ConsensusMetrics>);

impl Default for ConsensusMetricsWrapper {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsensusMetricsWrapper {
    pub fn new() -> Self {
        METRICS_INSTANCE
            .get_or_init(|| {
                Self(
                    METRICS_ENABLED
                        .get()
                        .copied()
                        .unwrap_or_default()
                        .then(ConsensusMetrics::default)
                )
            })
            .clone()
    }

    pub fn set_consensus_completion_time(&self, block_number: u64, time: u128) {
        if self.0.is_some() {
            publish_block_metrics_event(BlockMetricsEvent::ConsensusCompletionTime {
                block:   block_number,
                time_ms: u64::try_from(time).unwrap_or(u64::MAX)
            });
        }
    }

    pub fn set_proposal_verification_time(&self, block_number: u64, time: u128) {
        if self.0.is_some() {
            publish_block_metrics_event(BlockMetricsEvent::ProposalVerificationTime {
                block:   block_number,
                time_ms: u64::try_from(time).unwrap_or(u64::MAX)
            });
        }
    }

    pub fn set_proposal_build_time(&self, block_number: u64, time: u128) {
        if self.0.is_some() {
            publish_block_metrics_event(BlockMetricsEvent::ProposalBuildTime {
                block:   block_number,
                time_ms: u64::try_from(time).unwrap_or(u64::MAX)
            });
        }
    }

    pub fn set_block_height(&self, block_number: u64) {
        if let Some(this) = self.0.as_ref() {
            this.set_block_height(block_number);
        }
    }

    pub fn set_commit_time(&self, _block_number: u64) {
        // No-op: per-block commit timings are written explicitly from consensus
        // state transitions.
    }
}
