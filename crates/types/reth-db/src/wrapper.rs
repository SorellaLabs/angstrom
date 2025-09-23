use std::sync::{Arc, atomic::AtomicU64};

// Allows us to impl revm::DatabaseRef on the default provider type.
use alloy::{
    primitives::{Address, B256, BlockHash, BlockNumber, Bytes, StorageKey, StorageValue, U256},
    transports::{RpcError, TransportErrorKind}
};
use reth_chainspec::ChainInfo;
use reth_provider::{
    AccountReader, BlockHashReader, BlockIdReader, BlockNumReader, BytecodeReader,
    HashedPostStateProvider, ProviderError, ProviderResult, StateProofProvider, StateProvider,
    StateProviderFactory
};
use reth_storage_api::{StateRootProvider, StorageRootProvider};
use reth_trie::{
    AccountProof, HashedPostState, HashedStorage, MultiProof, StorageMultiProof, TrieInput,
    updates::TrieUpdates
};
use revm::state::AccountInfo;
use revm_bytecode::Bytecode;
use revm_database::{BundleState, DBErrorMarker};

pub trait SetBlock: Send + Sync + 'static {
    fn set_block(&self, block: u64);
}

#[derive(Clone)]
pub struct RethDbWrapper<DB: StateProviderFactory + Unpin + Clone + 'static> {
    db:    DB,
    block: Arc<AtomicU64>
}

impl<DB: StateProviderFactory + Unpin + Clone + 'static> SetBlock for RethDbWrapper<DB> {
    fn set_block(&self, block: u64) {
        self.block
            .store(block, std::sync::atomic::Ordering::Relaxed);
    }
}

impl<DB> RethDbWrapper<DB>
where
    DB: StateProviderFactory + Unpin + Clone + 'static
{
    pub fn new(db: DB, block: u64) -> Self {
        Self { db, block: Arc::new(block.into()) }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DBError {
    #[error(transparent)]
    Regular(#[from] ProviderError),
    #[error(transparent)]
    Eyre(#[from] eyre::Error),
    #[error("{0:?}")]
    String(String),
    #[error(transparent)]
    Rpc(#[from] RpcError<TransportErrorKind>)
}

impl DBErrorMarker for DBError {}

impl<DB> revm::DatabaseRef for RethDbWrapper<DB>
where
    DB: StateProviderFactory + Unpin + Clone + 'static
{
    type Error = DBError;

    /// Retrieves basic account information for a given address.
    ///
    /// Returns `Ok` with `Some(AccountInfo)` if the account exists,
    /// `None` if it doesn't, or an error if encountered.
    fn basic_ref(&self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        Ok(self.basic_account(&address)?.map(Into::into))
    }

    /// Retrieves the bytecode associated with a given code hash.
    ///
    /// Returns `Ok` with the bytecode if found, or the default bytecode
    /// otherwise.
    fn code_by_hash_ref(&self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        Ok(self.bytecode_by_hash(&code_hash)?.unwrap_or_default().0)
    }

    /// Retrieves the storage value at a specific index for a given address.
    ///
    /// Returns `Ok` with the storage value, or the default value if not found.
    fn storage_ref(&self, address: Address, index: U256) -> Result<U256, Self::Error> {
        Ok(self
            .storage(address, B256::new(index.to_be_bytes()))?
            .unwrap_or_default())
    }

    /// Retrieves the block hash for a given block number.
    ///
    /// Returns `Ok` with the block hash if found, or the default hash
    /// otherwise.
    fn block_hash_ref(&self, number: u64) -> Result<B256, Self::Error> {
        // Get the block hash or default hash with an attempt to convert U256 block
        // number to u64
        Ok(self.db.block_hash(number)?.unwrap_or_default())
    }
}

impl<DB> BlockNumReader for RethDbWrapper<DB>
where
    DB: StateProviderFactory + Unpin + Clone + 'static
{
    fn chain_info(&self) -> reth_provider::ProviderResult<ChainInfo> {
        self.db.chain_info()
    }

    fn block_number(&self, hash: B256) -> reth_provider::ProviderResult<Option<BlockNumber>> {
        self.db.block_number(hash)
    }

    fn convert_number(
        &self,
        id: alloy::eips::BlockHashOrNumber
    ) -> reth_provider::ProviderResult<Option<B256>> {
        self.db.convert_number(id)
    }

    fn best_block_number(&self) -> reth_provider::ProviderResult<BlockNumber> {
        self.db.best_block_number()
    }

    fn last_block_number(&self) -> reth_provider::ProviderResult<BlockNumber> {
        self.db.last_block_number()
    }

    fn convert_hash_or_number(
        &self,
        id: alloy::eips::BlockHashOrNumber
    ) -> reth_provider::ProviderResult<Option<BlockNumber>> {
        self.db.convert_hash_or_number(id)
    }
}

impl<DB> BlockIdReader for RethDbWrapper<DB>
where
    DB: StateProviderFactory + Unpin + Clone + 'static
{
    fn pending_block_num_hash(&self) -> ProviderResult<Option<alloy::eips::BlockNumHash>> {
        self.db.pending_block_num_hash()
    }

    fn safe_block_num_hash(&self) -> ProviderResult<Option<alloy::eips::BlockNumHash>> {
        self.db.safe_block_num_hash()
    }

    fn finalized_block_num_hash(&self) -> ProviderResult<Option<alloy::eips::BlockNumHash>> {
        self.db.finalized_block_num_hash()
    }
}

impl<DB> StateProviderFactory for RethDbWrapper<DB>
where
    DB: StateProviderFactory + Unpin + Clone + 'static
{
    fn maybe_pending(&self) -> ProviderResult<Option<reth_provider::StateProviderBox>> {
        self.db.maybe_pending()
    }

    fn latest(&self) -> reth_provider::ProviderResult<reth_provider::StateProviderBox> {
        self.db.latest()
    }

    fn pending(&self) -> reth_provider::ProviderResult<reth_provider::StateProviderBox> {
        self.db.pending()
    }

    fn state_by_block_id(
        &self,
        block_id: alloy::eips::BlockId
    ) -> reth_provider::ProviderResult<reth_provider::StateProviderBox> {
        self.db.state_by_block_id(block_id)
    }

    fn state_by_block_hash(
        &self,
        block: BlockHash
    ) -> reth_provider::ProviderResult<reth_provider::StateProviderBox> {
        self.db.state_by_block_hash(block)
    }

    fn history_by_block_hash(
        &self,
        block: BlockHash
    ) -> reth_provider::ProviderResult<reth_provider::StateProviderBox> {
        self.db.history_by_block_hash(block)
    }

    fn pending_state_by_hash(
        &self,
        block_hash: B256
    ) -> reth_provider::ProviderResult<Option<reth_provider::StateProviderBox>> {
        self.db.pending_state_by_hash(block_hash)
    }

    fn state_by_block_number_or_tag(
        &self,
        number_or_tag: alloy::eips::BlockNumberOrTag
    ) -> reth_provider::ProviderResult<reth_provider::StateProviderBox> {
        self.db.state_by_block_number_or_tag(number_or_tag)
    }

    fn history_by_block_number(
        &self,
        block: BlockNumber
    ) -> reth_provider::ProviderResult<reth_provider::StateProviderBox> {
        self.db.history_by_block_number(block)
    }
}

impl<DB> StateProvider for RethDbWrapper<DB>
where
    DB: StateProviderFactory + Unpin + Clone + 'static
{
    fn storage(
        &self,
        account: Address,
        storage_key: StorageKey
    ) -> reth_provider::ProviderResult<Option<StorageValue>> {
        self.db
            .state_by_block_id(self.block.load(std::sync::atomic::Ordering::Relaxed).into())?
            .storage(account, storage_key)
    }

    fn account_code(
        &self,
        addr: &Address
    ) -> reth_provider::ProviderResult<Option<reth_primitives::Bytecode>> {
        self.db
            .state_by_block_id(self.block.load(std::sync::atomic::Ordering::Relaxed).into())?
            .account_code(addr)
    }

    fn account_nonce(&self, addr: &Address) -> reth_provider::ProviderResult<Option<u64>> {
        self.db
            .state_by_block_id(self.block.load(std::sync::atomic::Ordering::Relaxed).into())?
            .account_nonce(addr)
    }

    fn account_balance(&self, addr: &Address) -> reth_provider::ProviderResult<Option<U256>> {
        self.db
            .state_by_block_id(self.block.load(std::sync::atomic::Ordering::Relaxed).into())?
            .account_balance(addr)
    }
}

impl<DB> AccountReader for RethDbWrapper<DB>
where
    DB: StateProviderFactory + Unpin + Clone + 'static
{
    fn basic_account(
        &self,
        address: &Address
    ) -> reth_provider::ProviderResult<Option<reth_primitives::Account>> {
        self.db
            .state_by_block_id(self.block.load(std::sync::atomic::Ordering::Relaxed).into())?
            .basic_account(address)
    }
}

impl<DB> BlockHashReader for RethDbWrapper<DB>
where
    DB: StateProviderFactory + Unpin + Clone + 'static
{
    fn block_hash(&self, number: BlockNumber) -> reth_provider::ProviderResult<Option<B256>> {
        self.db.latest()?.block_hash(number)
    }

    fn convert_block_hash(
        &self,
        hash_or_number: alloy::eips::BlockHashOrNumber
    ) -> reth_provider::ProviderResult<Option<B256>> {
        self.db.latest()?.convert_block_hash(hash_or_number)
    }

    fn canonical_hashes_range(
        &self,
        start: BlockNumber,
        end: BlockNumber
    ) -> reth_provider::ProviderResult<Vec<B256>> {
        self.db.latest()?.canonical_hashes_range(start, end)
    }
}

impl<DB> HashedPostStateProvider for RethDbWrapper<DB>
where
    DB: StateProviderFactory + Unpin + Clone + 'static
{
    fn hashed_post_state(&self, bundle_state: &BundleState) -> HashedPostState {
        self.db.latest().unwrap().hashed_post_state(bundle_state)
    }
}

impl<DB> StateRootProvider for RethDbWrapper<DB>
where
    DB: StateProviderFactory + Unpin + Clone + 'static
{
    fn state_root(&self, hashed_state: HashedPostState) -> reth_provider::ProviderResult<B256> {
        self.db.latest()?.state_root(hashed_state)
    }

    fn state_root_from_nodes(&self, input: TrieInput) -> reth_provider::ProviderResult<B256> {
        self.db.latest()?.state_root_from_nodes(input)
    }

    fn state_root_with_updates(
        &self,
        hashed_state: HashedPostState
    ) -> reth_provider::ProviderResult<(B256, TrieUpdates)> {
        self.db.latest()?.state_root_with_updates(hashed_state)
    }

    fn state_root_from_nodes_with_updates(
        &self,
        input: TrieInput
    ) -> reth_provider::ProviderResult<(B256, TrieUpdates)> {
        self.db.latest()?.state_root_from_nodes_with_updates(input)
    }
}

impl<DB> StorageRootProvider for RethDbWrapper<DB>
where
    DB: StateProviderFactory + Unpin + Clone + 'static
{
    fn storage_proof(
        &self,
        address: Address,
        slot: B256,
        hashed_storage: HashedStorage
    ) -> ProviderResult<reth_trie::StorageProof> {
        self.db
            .latest()?
            .storage_proof(address, slot, hashed_storage)
    }

    fn storage_root(
        &self,
        address: Address,
        hashed_storage: HashedStorage
    ) -> ProviderResult<B256> {
        self.db.latest()?.storage_root(address, hashed_storage)
    }

    fn storage_multiproof(
        &self,
        address: Address,
        slots: &[B256],
        hashed_storage: HashedStorage
    ) -> ProviderResult<StorageMultiProof> {
        self.db
            .latest()?
            .storage_multiproof(address, slots, hashed_storage)
    }
}

impl<DB> StateProofProvider for RethDbWrapper<DB>
where
    DB: StateProviderFactory + Unpin + Clone + 'static
{
    fn proof(
        &self,
        input: TrieInput,
        address: Address,
        slots: &[B256]
    ) -> reth_provider::ProviderResult<AccountProof> {
        self.db.latest()?.proof(input, address, slots)
    }

    fn witness(&self, input: TrieInput, target: HashedPostState) -> ProviderResult<Vec<Bytes>> {
        self.db.latest()?.witness(input, target)
    }

    fn multiproof(
        &self,
        input: TrieInput,
        targets: reth_trie::MultiProofTargets
    ) -> ProviderResult<MultiProof> {
        self.db.latest()?.multiproof(input, targets)
    }
}

impl<DB> BytecodeReader for RethDbWrapper<DB>
where
    DB: StateProviderFactory + Unpin + Clone + 'static
{
    fn bytecode_by_hash(
        &self,
        code_hash: &B256
    ) -> reth_provider::ProviderResult<Option<reth_primitives::Bytecode>> {
        self.db
            .state_by_block_id(self.block.load(std::sync::atomic::Ordering::Relaxed).into())?
            .bytecode_by_hash(code_hash)
    }
}
