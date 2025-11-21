use alloy_primitives::{FixedBytes, U256};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GasEstimateUpdate {
    timestamp:        u128,
    pair:             FixedBytes<32>,
    estimate_wei:     u64,
    old_estimate_erc: U256,
    new_estimate_erc: U256
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, Default,
)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub enum GasEstimateFilter {
    /// will give updates for all pools
    #[default]
    None,
    Pair(FixedBytes<32>)
}
