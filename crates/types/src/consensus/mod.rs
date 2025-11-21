pub mod evidence;
pub mod pre_prepose;
pub mod pre_propose_agg;
pub mod proposal;
pub mod round_data;
use std::time::Duration;

use alloy::primitives::{Address, BlockNumber, Bytes};
pub use evidence::*;
pub use pre_prepose::*;
pub use pre_propose_agg::*;
pub mod slot_clock;
pub use proposal::*;
pub use round_data::*;
use serde::{Deserialize, Serialize};
pub use slot_clock::*;

/// do the math with fixed here to avoid floats
pub const ONE_E3: u64 = 1000;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsensusRoundName {
    BidAggregation,
    Finalization,
    PreProposalAggregation,
    PreProposal,
    Proposal
}

impl ConsensusRoundName {
    pub fn is_closed(&self) -> bool {
        !matches!(self, ConsensusRoundName::BidAggregation)
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum StromConsensusEvent {
    PreProposal(Address, PreProposal),
    PreProposalAgg(Address, PreProposalAggregation),
    Proposal(Address, Proposal),
    BundleUnlockAttestation(Address, u64, Bytes)
}

impl StromConsensusEvent {
    pub fn message_type(&self) -> &'static str {
        match self {
            StromConsensusEvent::PreProposal(..) => "PreProposal",
            StromConsensusEvent::PreProposalAgg(..) => "PreProposalAggregation",
            StromConsensusEvent::Proposal(..) => "Proposal",
            StromConsensusEvent::BundleUnlockAttestation(..) => "BundleUnlockAttestation"
        }
    }

    pub fn sender(&self) -> Address {
        match self {
            StromConsensusEvent::PreProposal(peer_id, _)
            | StromConsensusEvent::Proposal(peer_id, _)
            | StromConsensusEvent::PreProposalAgg(peer_id, _)
            | StromConsensusEvent::BundleUnlockAttestation(peer_id, ..) => *peer_id
        }
    }

    pub fn block_height(&self) -> BlockNumber {
        match self {
            StromConsensusEvent::PreProposal(_, PreProposal { block_height, .. }) => *block_height,
            StromConsensusEvent::PreProposalAgg(_, p) => p.block_height,
            StromConsensusEvent::Proposal(_, Proposal { block_height, .. }) => *block_height,
            StromConsensusEvent::BundleUnlockAttestation(_, block, _) => *block
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AngstromValidator {
    pub peer_id:      Address,
    pub voting_power: u64,
    pub priority:     i64
}

impl AngstromValidator {
    pub fn new(name: Address, voting_power: u64) -> Self {
        AngstromValidator {
            peer_id:      name,
            voting_power: voting_power * ONE_E3,
            priority:     0
        }
    }
}

impl PartialEq for AngstromValidator {
    fn eq(&self, other: &Self) -> bool {
        self.peer_id == other.peer_id
    }
}

impl Eq for AngstromValidator {}

impl std::hash::Hash for AngstromValidator {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.peer_id.hash(state);
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ConsensusDataWithBlock<T> {
    pub data:  T,
    pub block: u64
}

#[derive(Debug, Clone, Copy, clap::Args, Serialize, Deserialize)]
pub struct ConsensusTimingConfig {
    #[clap(long, default_value_t = 8_000)]
    pub min_wait_duration_ms: u64,
    #[clap(long, default_value_t = 9_000)]
    pub max_wait_duration_ms: u64
}

impl Default for ConsensusTimingConfig {
    fn default() -> Self {
        Self { min_wait_duration_ms: 8_000, max_wait_duration_ms: 9_000 }
    }
}

impl ConsensusTimingConfig {
    pub fn is_valid(&self) -> bool {
        self.min_wait_duration_ms < self.max_wait_duration_ms
    }

    pub const fn min_wait_time_ms(&self) -> Duration {
        Duration::from_millis(self.min_wait_duration_ms)
    }

    pub const fn max_wait_time_ms(&self) -> Duration {
        Duration::from_millis(self.max_wait_duration_ms)
    }

    pub fn default_duration(&self) -> Duration {
        Duration::from_secs_f64(
            (self.max_wait_time_ms() + self.min_wait_time_ms()).as_secs_f64() / 2.0
        )
    }
}
