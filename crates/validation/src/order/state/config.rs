use std::path::Path;

use alloy::primitives::Address;
use angstrom_types::primitive::PoolId;
use reth_primitives::{keccak256, U256};
use reth_revm::DatabaseRef;
use serde::Deserialize;

use crate::common::lru_db::{BlockStateProviderFactory, RevmLRU};
#[derive(Debug, Clone, Deserialize)]
pub struct DataFetcherConfig {
    pub approvals: Vec<TokenApprovalSlot>,
    pub balances:  Vec<TokenBalanceSlot>
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct ValidationConfig {
    pub pools:                   Vec<PoolConfig>,
    pub max_validation_per_user: usize
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
pub struct PoolConfig {
    pub token0:  Address,
    pub token1:  Address,
    pub pool_id: PoolId
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenBalanceSlot {
    pub token:       Address,
    pub hash_method: HashMethod,
    pub slot_index:  u8
}

impl TokenBalanceSlot {
    pub fn generate_slot(&self, of: Address) -> eyre::Result<U256> {
        if !self.hash_method.is_solidity() {
            return Err(eyre::eyre!("current type of contract hashing is not supported"))
        }

        let mut buf = [0u8; 64];
        buf[12..32].copy_from_slice(&**of);
        buf[63] = self.slot_index;

        Ok(U256::from_be_bytes(*keccak256(buf)))
    }

    pub fn load_balance<DB: BlockStateProviderFactory>(
        &self,
        of: Address,
        db: &RevmLRU<DB>
    ) -> eyre::Result<U256> {
        Ok(db.storage_ref(self.token, self.generate_slot(of)?)?)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenApprovalSlot {
    pub token:       Address,
    pub hash_method: HashMethod,
    pub slot_index:  u8
}

impl TokenApprovalSlot {
    pub fn generate_slot(&self, user: Address, contract: Address) -> eyre::Result<U256> {
        if !self.hash_method.is_solidity() {
            return Err(eyre::eyre!("current type of contract hashing is not supported"))
        }
        let mut inner_buf = [0u8; 64];
        inner_buf[12..32].copy_from_slice(&**contract);
        inner_buf[63] = self.slot_index;
        let inner_hash = keccak256(inner_buf);
        let mut next = [0u8; 64];
        next[12..32].copy_from_slice(&**user);
        next[32..64].copy_from_slice(&*inner_hash);

        Ok(U256::from_be_bytes(*keccak256(next)))
    }

    pub fn load_approval_amount<DB: BlockStateProviderFactory>(
        &self,
        user: Address,
        contract: Address,
        db: &RevmLRU<DB>
    ) -> eyre::Result<U256> {
        if !self.hash_method.is_solidity() {
            return Err(eyre::eyre!("current type of contract hashing is not supported"))
        }

        Ok(db.storage_ref(self.token, self.generate_slot(user, contract)?)?)
    }
}

pub fn load_data_fetcher_config(config_path: &Path) -> eyre::Result<DataFetcherConfig> {
    let file = std::fs::read_to_string(config_path)?;
    Ok(toml::from_str(&file)?)
}

pub fn load_validation_config(config_path: &Path) -> eyre::Result<ValidationConfig> {
    let file = std::fs::read_to_string(config_path)?;
    Ok(toml::from_str(&file)?)
}
