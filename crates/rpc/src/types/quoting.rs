use alloy_primitives::U256;
use angstrom_types::contract_bindings::angstrom::Angstrom::PoolKey;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct BBO {
    pub pool:   PoolKey,
    pub bid:    U256,
    pub bid_am: U256,
    pub ask:    U256,
    pub ask_am: U256
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Depth5 {
    pub pool:   PoolKey,
    pub bids:   [U256; 5],
    pub bid_am: [U256; 5],
    pub ask:    [U256; 5],
    pub ask_am: [U256; 5]
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Depth25 {
    pub pool:   PoolKey,
    pub bids:   [U256; 25],
    pub bid_am: [U256; 25],
    pub ask:    [U256; 25],
    pub ask_am: [U256; 25]
}
