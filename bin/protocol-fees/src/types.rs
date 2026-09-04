use alloy_primitives::Address;
use angstrom_types_primitives::{ANGSTROM_ADDRESS, ANGSTROM_DEPLOYED_BLOCK, CONTROLLER_V1_ADDRESS};

#[derive(Debug, Clone, Copy)]
pub struct ProtocolFeeBlockRange {
    /// the tx index AFTER the previous range's protocol fee change
    pub start_block:  BlockAndTxIndex,
    /// the tx index OF the current range's protocol fee change
    pub end_block:    BlockAndTxIndex,
    pub protocol_fee: u64
}

#[derive(Debug, Clone, Copy)]
pub struct BlockAndTxIndex {
    pub block_number: u64,
    pub tx_index:     u64
}

pub fn angstrom_deployed_block() -> u64 {
    *ANGSTROM_DEPLOYED_BLOCK.get().unwrap()
}

pub fn controller_v1_address() -> Address {
    *CONTROLLER_V1_ADDRESS.get().unwrap()
}

pub fn angstrom_address() -> Address {
    *ANGSTROM_ADDRESS.get().unwrap()
}
