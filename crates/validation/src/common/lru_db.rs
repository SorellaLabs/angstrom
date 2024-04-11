use std::{
    collections::HashMap,
    sync::{atomic::AtomicU64, Arc}
};

use alloy_primitives::Address;
use parking_lot::RwLock;
use reth_interfaces::RethError;
use reth_primitives::{
    revm_primitives::{AccountInfo, Bytecode, B256, U256},
    KECCAK_EMPTY
};
use reth_provider::{AccountReader, StateProvider, StateProviderFactory};
use reth_revm::{Database, DatabaseRef};
use revm::db::DbAccount;
use schnellru::{ByMemoryUsage, LruMap};

use crate::{
    bundle::errors::SimError,
    common::state::{AddressSlots, RevmBackend}
};

pub struct RevmLRU<DB> {
    state_overrides:    HashMap<Address, HashMap<U256, U256>>,
    bytecode_overrides: HashMap<Address, Bytecode>,
    accounts:           Arc<RwLock<LruMap<Address, DbAccount, ByMemoryUsage>>>,
    contracts:          Arc<RwLock<LruMap<B256, Bytecode, ByMemoryUsage>>>,
    db:                 Arc<DB>,
    current_block:      Arc<AtomicU64>
}

impl<DB: Clone> Clone for RevmLRU<DB> {
    fn clone(&self) -> Self {
        Self {
            state_overrides:    HashMap::default(),
            bytecode_overrides: HashMap::default(),
            accounts:           self.accounts.clone(),
            contracts:          self.contracts.clone(),
            db:                 self.db.clone(),
            current_block:      self.current_block.clone()
        }
    }
}

impl<DB> RevmBackend for RevmLRU<DB>
where
    DB: StateProviderFactory
{
    fn update_evm_state(&self, slot_changes: &AddressSlots) -> eyre::Result<(), SimError> {
        let mut accounts = self.accounts.write();

        for (addr, storage) in slot_changes.iter() {
            let acct_storage = accounts
                .get_or_insert(*addr, || DbAccount {
                    info: self.basic_ref_no_cache(addr).unwrap().unwrap(),
                    ..Default::default()
                })
                .unwrap();
            for (idx, val) in storage {
                let new_state = self.storage_ref_no_cache(addr, *idx)?;
                if new_state != *val {
                    acct_storage.storage.insert(*idx, new_state);
                }
            }
        }
        Ok(())
    }
}

impl<DB> RevmLRU<DB>
where
    DB: StateProviderFactory
{
    pub fn new(max_bytes: usize, db: Arc<DB>, current_block: Arc<AtomicU64>) -> Self {
        let accounts = Arc::new(RwLock::new(LruMap::new(ByMemoryUsage::new(max_bytes))));
        let contracts = Arc::new(RwLock::new(LruMap::new(ByMemoryUsage::new(max_bytes))));
        Self {
            current_block,
            accounts,
            contracts,
            db,
            state_overrides: HashMap::default(),
            bytecode_overrides: HashMap::default()
        }
    }

    pub fn update_block_number(&self, block_number: u64) {
        self.current_block
            .store(block_number, std::sync::atomic::Ordering::SeqCst)
    }

    pub fn set_state_overrides(&mut self, overrides: HashMap<Address, HashMap<U256, U256>>) {
        self.state_overrides = overrides
    }

    pub fn set_bytecode_overrides(&mut self, overrides: HashMap<Address, Bytecode>) {
        self.bytecode_overrides = overrides
    }

    fn basic_ref_no_cache(&self, address: &Address) -> Result<Option<AccountInfo>, RethError> {
        Ok(self
            .db
            .latest()?
            .basic_account(*address)?
            .map(|account| AccountInfo {
                balance:   account.balance,
                nonce:     account.nonce,
                code_hash: account.bytecode_hash.unwrap_or(KECCAK_EMPTY),
                code:      None
            }))
    }

    fn storage_ref_no_cache(&self, address: &Address, index: U256) -> Result<U256, RethError> {
        self.db
            .latest()?
            .storage(*address, index.into())
            .map(|inner| inner.unwrap_or_default())
            .map_err(RethError::from)
    }
}

impl<DB> Database for RevmLRU<DB>
where
    DB: StateProviderFactory
{
    type Error = RethError;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        DatabaseRef::basic_ref(&self, address)
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        DatabaseRef::storage_ref(&self, address, index)
    }

    fn block_hash(&mut self, number: U256) -> Result<B256, Self::Error> {
        DatabaseRef::block_hash_ref(&self, number)
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        DatabaseRef::code_by_hash_ref(&self, code_hash)
    }
}

impl<DB> DatabaseRef for RevmLRU<DB>
where
    DB: StateProviderFactory
{
    type Error = RethError;

    fn basic_ref(&self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        let mut accounts = self.accounts.write();

        accounts
            .get(&address)
            .map(|acc| Ok(acc.info()))
            .unwrap_or_else(|| self.basic_ref_no_cache(&address))
    }

    fn code_by_hash_ref(&self, _code_hash: B256) -> Result<Bytecode, Self::Error> {
        unreachable!() // this should never be reached since the code hash is
                       // defined in basic()
    }

    fn storage_ref(&self, address: Address, index: U256) -> Result<U256, Self::Error> {
        // check for overrides
        if let Some(storage) = self.state_overrides.get(&address) {
            if let Some(value) = storage.get(&index) {
                return Ok(*value)
            }
        }

        let mut accounts = self.accounts.write();

        Ok(accounts
            .get(&address)
            .map(|account_entry| {
                account_entry
                    .storage
                    .get(&index)
                    .map(|e| Ok(Some(*e)))
                    .unwrap_or_else(|| self.db.latest()?.storage(address, index.into()))
            })
            .unwrap_or_else(|| self.db.latest()?.storage(address, index.into()))?
            .unwrap_or_default())
    }

    fn block_hash_ref(&self, _number: U256) -> Result<B256, Self::Error> {
        unreachable!() // this should never be reached since we will never sim
                       // blocks
    }
}
