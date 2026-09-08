use alloy_eips::BlockNumHash;
use alloy_primitives::Address;
use angstrom_types_primitives::{ANGSTROM_ADDRESS, ANGSTROM_DEPLOYED_BLOCK, CONTROLLER_V1_ADDRESS};

pub fn angstrom_deployed_block() -> u64 {
    *ANGSTROM_DEPLOYED_BLOCK.get().unwrap()
}

pub fn controller_v1_address() -> Address {
    *CONTROLLER_V1_ADDRESS.get().unwrap()
}

pub fn angstrom_address() -> Address {
    *ANGSTROM_ADDRESS.get().unwrap()
}

pub struct TokenSavings {
    pub asset:       Address,
    pub symbol:      String,
    pub saved_gross: String
}

pub struct BundleFees {
    pub block:  BlockNumHash,
    pub tokens: Vec<TokenSavings>
}
