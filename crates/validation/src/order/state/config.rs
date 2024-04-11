use std::path::Path;

use alloy_primitives::Address;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct TokenSlotConfig {
    pub approvals: Vec<TokenApprovalSlot>,
    pub balances:  Vec<TokenBalanceSlot>
}

#[derive(Debug, Clone, Deserialize)]
pub enum HashMethod {
    #[serde(rename = "sol")]
    Solidity,
    #[serde(rename = "vyper")]
    Vyper
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenBalanceSlot {
    token:       Address,
    hash_method: HashMethod,
    slot_index:  u8
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenApprovalSlot {
    token:       Address,
    hash_method: HashMethod,
    slot_index:  u8
}

pub fn load_token_slot_config(config_path: &Path) -> eyre::Result<TokenSlotConfig> {
    let file = std::fs::read_to_string(config_path)?;
    let approvals: Vec<TokenApprovalSlot> = toml::from_str(&file)?;
    let balances: Vec<TokenBalanceSlot> = toml::from_str(&file)?;
    Ok(TokenSlotConfig { approvals, balances })
}
