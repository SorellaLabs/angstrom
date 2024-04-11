use std::{path::Path, sync::Arc};

use alloy_primitives::Address;
use alloy_sol_types::SolValue;
use reth_primitives::{keccak256, U256};
use reth_provider::StateProviderFactory;
use revm::DatabaseRef;
use serde::Deserialize;

use crate::common::lru_db::RevmLRU;

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
impl HashMethod {
    const fn is_solidity(&self) -> bool {
        matches!(self, HashMethod::Solidity)
    }

    const fn is_vyper(&self) -> bool {
        matches!(self, HashMethod::Vyper)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenBalanceSlot {
    token:       Address,
    hash_method: HashMethod,
    slot_index:  U256
}

impl TokenBalanceSlot {
    pub fn generate_slot(&self, of: Address) -> eyre::Result<U256> {
        if !self.hash_method.is_solidity() {
            return Err(eyre::eyre!("current type of contract hashing is not supported"))
        }

        let mut buf = [0u8; 64];
        buf[12..32].copy_from_slice(&**of);
        buf[32..64].copy_from_slice(&self.slot_index.to_be_bytes::<32>());

        Ok(U256::from_be_bytes(*keccak256(buf)))
    }

    pub fn load_balance<DB: StateProviderFactory>(
        &self,
        of: Address,
        db: Arc<RevmLRU<DB>>
    ) -> eyre::Result<U256> {
        Ok(db.storage_ref(self.token, self.generate_slot(of)?)?)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenApprovalSlot {
    token:       Address,
    hash_method: HashMethod,
    slot_index:  U256
}

impl TokenApprovalSlot {
    pub fn generate_slot(&self, user: Address, contract: Address) -> eyre::Result<U256> {
        if !self.hash_method.is_solidity() {
            return Err(eyre::eyre!("current type of contract hashing is not supported"))
        }
        let mut inner_buf = [0u8; 64];
        inner_buf[12..32].copy_from_slice(&**contract);
        inner_buf[32..64].copy_from_slice(&self.slot_index.to_be_bytes::<32>());
        let inner_hash = keccak256(inner_buf);
        let mut next = [0u8; 64];
        next[12..32].copy_from_slice(&**user);
        next[32..64].copy_from_slice(&*inner_hash);

        Ok(U256::from_be_bytes(*keccak256(next)))
    }

    pub fn load_approval_amount<DB: StateProviderFactory>(
        &self,
        user: Address,
        contract: Address,
        db: Arc<RevmLRU<DB>>
    ) -> eyre::Result<U256> {
        if !self.hash_method.is_solidity() {
            return Err(eyre::eyre!("current type of contract hashing is not supported"))
        }

        Ok(db.storage_ref(self.token, self.generate_slot(user, contract)?)?)
    }
}

pub fn load_token_slot_config(config_path: &Path) -> eyre::Result<TokenSlotConfig> {
    let file = std::fs::read_to_string(config_path)?;
    let approvals: Vec<TokenApprovalSlot> = toml::from_str(&file)?;
    let balances: Vec<TokenBalanceSlot> = toml::from_str(&file)?;
    Ok(TokenSlotConfig { approvals, balances })
}
