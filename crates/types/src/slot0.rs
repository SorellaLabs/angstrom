use alloy_primitives::U160;
use angstrom_constants::PoolId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct Slot0Update {
    /// there will be 120 updates per block or per 100ms
    pub seq_id:           u16,
    /// in case of block lag on node
    pub current_block:    u64,
    pub angstrom_pool_id: PoolId,
    pub uni_pool_id:      PoolId,

    pub sqrt_price_x96: U160,
    pub liquidity:      u128,
    pub tick:           i32
}
