use alloy_primitives::Address;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MetricsEventEnvelope {
    /// Hex signer address of the producing node.
    pub node_address:        Address,
    /// Chain id associated with the node at event production time.
    pub chain_id:            u64,
    /// Producer-side event timestamp in unix milliseconds.
    /// This avoids drift from downstream queueing/retry delays.
    pub observed_at_unix_ms: u64,
    pub event:               MetricsEvent
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MetricsEvent {
    BlockMetrics(BlockMetricsEvent),
    ConsensusMetrics(ConsensusMetricsEvent)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BlockMetricsEvent {
    PreproposalOrders {
        block:    u64,
        limit:    usize,
        searcher: usize
    },
    StateTransition {
        block:          u64,
        state:          String,
        slot_offset_ms: u64,
        limit:          usize,
        searcher:       usize
    },
    PreproposalsCollected {
        block: u64,
        count: usize
    },
    IsLeader {
        block:     u64,
        is_leader: bool
    },
    MatchingInputPreQuorum {
        block:    u64,
        limit:    usize,
        searcher: usize
    },
    MatchingInputPostQuorum {
        block:    u64,
        limit:    usize,
        searcher: usize
    },
    MatchingResults {
        block:            u64,
        pools_solved:     usize,
        filled:           usize,
        partial:          usize,
        unfilled:         usize,
        killed:           usize,
        bundle_generated: bool
    },
    SubmissionStarted {
        block:          u64,
        slot_offset_ms: u64
    },
    SubmissionCompleted {
        block:          u64,
        slot_offset_ms: u64,
        latency_ms:     u64,
        success:        bool
    },
    SubmissionEndpoint {
        block:          u64,
        submitter_type: String,
        endpoint:       String,
        success:        bool,
        latency_ms:     u64
    },
    BundleIncluded {
        block:    u64,
        included: bool
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConsensusMetricsEvent {
    ProposalBuildTime { block: u64, time_ms: u64 },
    ProposalVerificationTime { block: u64, time_ms: u64 },
    ConsensusCompletionTime { block: u64, time_ms: u64 }
}

impl From<BlockMetricsEvent> for MetricsEvent {
    fn from(value: BlockMetricsEvent) -> Self {
        Self::BlockMetrics(value)
    }
}

impl From<ConsensusMetricsEvent> for MetricsEvent {
    fn from(value: ConsensusMetricsEvent) -> Self {
        Self::ConsensusMetrics(value)
    }
}
