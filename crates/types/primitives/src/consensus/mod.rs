mod pre_proposal;
use std::time::Duration;

use alloy_primitives::Address;
pub use pre_proposal::*;

mod proposal;
pub use proposal::*;

mod pre_proposal_agg;
pub use pre_proposal_agg::*;

/// do the math with fixed here to avoid floats
pub const ONE_E3: u64 = 1000;

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

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct ConsensusDataWithBlock<T> {
    pub data:  T,
    pub block: u64
}

#[derive(Debug, Clone, Copy, clap::Args, serde::Serialize, serde::Deserialize)]
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
