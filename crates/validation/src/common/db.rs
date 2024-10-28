use std::{
    collections::HashMap,
    sync::{atomic::AtomicU64, Arc}
};

use alloy::primitives::{Address, BlockNumber, StorageKey, StorageValue};
use parking_lot::RwLock;
use reth_errors::{ProviderError, RethError, RethResult};
use reth_primitives::{
    revm_primitives::{AccountInfo, Bytecode, B256, U256},
    Account, KECCAK_EMPTY
};
use reth_provider::{
    AccountReader, BlockNumReader, ProviderResult, StateProvider, StateProviderBox,
    StateProviderFactory
};
use reth_revm::{Database, DatabaseRef};
use revm::db::DbAccount;
use schnellru::{ByMemoryUsage, LruMap};

pub trait BlockStateProvider {
    fn get_basic_account(&self, address: Address) -> ProviderResult<Option<Account>>;

    fn get_storage(
        &self,
        address: Address,
        key: StorageKey
    ) -> ProviderResult<Option<StorageValue>>;
}

pub trait BlockStateProviderFactory: Send + Sync {
    type Provider: BlockStateProvider;

    fn state_by_block(&self, block: u64) -> ProviderResult<Self::Provider>;

    fn best_block_number(&self) -> ProviderResult<BlockNumber>;
}

impl BlockStateProvider for StateProviderBox {
    fn get_basic_account(&self, address: Address) -> ProviderResult<Option<Account>> {
        AccountReader::basic_account(self, address)
    }

    fn get_storage(
        &self,
        address: Address,
        key: StorageKey
    ) -> ProviderResult<Option<StorageValue>> {
        StateProvider::storage(&self, address, key)
    }
}

impl<T: StateProviderFactory> BlockStateProviderFactory for T {
    type Provider = StateProviderBox;

    fn state_by_block(&self, block: u64) -> ProviderResult<StateProviderBox> {
        self.state_by_block_id(block.into())
    }

    fn best_block_number(&self) -> ProviderResult<BlockNumber> {
        BlockNumReader::best_block_number(self)
    }
}
