use std::collections::HashMap;

use alloy_primitives::{FixedBytes, U256};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleEstimate {
    pub total_orders:  u64,
    pub pool_estimate: HashMap<FixedBytes<32>, PoolEstimate>
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolEstimate {
    pub orders:        u64,
    pub gas_in_wei:    u64,
    pub gas_in_token0: U256
}
