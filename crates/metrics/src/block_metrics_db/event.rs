#[derive(Debug, Clone)]
pub(crate) enum BlockMetricEvent {
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
    },
    ProposalBuildTime {
        block:   u64,
        time_ms: u128
    },
    ProposalVerificationTime {
        block:   u64,
        time_ms: u128
    },
    ConsensusCompletionTime {
        block:   u64,
        time_ms: u128
    }
}
