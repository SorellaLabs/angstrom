// Allows us to impl revm::DatabaseRef on the default provider type.
use alloy::{
    eips::BlockNumHash,
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
use revm::{primitives::KECCAK_EMPTY, state::AccountInfo};
use revm_bytecode::Bytecode;
use revm_database::{BundleState, DBErrorMarker};

/// A state source that hands out immutable views of itself, one per block.
///
/// A view never moves after construction: a different block is a different
/// view, so nothing one reader does can change what another one reads.
pub trait AtBlock: Send + Sync + 'static {
    fn at_block(&self, block: BlockNumHash) -> Self;
}

#[derive(Clone)]
pub struct RethDbWrapper<DB: StateProviderFactory + Unpin + Clone + 'static> {
    db:    DB,
    /// The block every read resolves against, fixed for the life of the view.
    /// A `BlockNumHash` rather than a number because a number cannot name one
    /// branch of a same-height reorg.
    block: BlockNumHash
}

impl<DB: StateProviderFactory + Unpin + Clone + 'static> AtBlock for RethDbWrapper<DB> {
    fn at_block(&self, block: BlockNumHash) -> Self {
        Self::new(self.db.clone(), block)
    }
}

impl<DB> RethDbWrapper<DB>
where
    DB: StateProviderFactory + Unpin + Clone + 'static
{
    pub fn new(db: DB, block: BlockNumHash) -> Self {
        Self { db, block }
    }

    /// The block reads resolve against.
    pub fn block(&self) -> BlockNumHash {
        self.block
    }

    /// The one place a state provider is resolved. Every read goes through it,
    /// so none of them can quietly answer from the current tip instead of the
    /// selected block.
    fn state(&self) -> ProviderResult<reth_provider::StateProviderBox> {
        self.db.state_by_block_id(self.block.hash.into())
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
    /// Absent bytecode is an error rather than the empty default: an account
    /// whose code we cannot read is not an account without code.
    fn code_by_hash_ref(&self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        // An account with genuinely no code is not a failed read.
        if code_hash == KECCAK_EMPTY {
            return Ok(Bytecode::default());
        }

        self.bytecode_by_hash(&code_hash)?
            .map(|code| code.0)
            .ok_or_else(|| DBError::String(format!("no bytecode for {code_hash}")))
    }

    /// Retrieves the storage value at a specific index for a given address.
    ///
    /// `None` here is an unset slot at a state that resolved, which the EVM
    /// reads as zero. State that did not resolve errors out of
    /// [`RethDbWrapper::state`], so the zero can no longer stand in for it.
    fn storage_ref(&self, address: Address, index: U256) -> Result<U256, Self::Error> {
        Ok(self
            .storage(address, B256::new(index.to_be_bytes()))?
            .unwrap_or_default())
    }

    /// Retrieves the block hash for a given block number.
    ///
    /// An unknown block is an error rather than the zero hash, which
    /// `BLOCKHASH` would otherwise read as a real answer.
    fn block_hash_ref(&self, number: u64) -> Result<B256, Self::Error> {
        self.db
            .block_hash(number)?
            .ok_or_else(|| DBError::String(format!("no block hash for {number}")))
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
        self.state()?.storage(account, storage_key)
    }

    fn account_code(
        &self,
        addr: &Address
    ) -> reth_provider::ProviderResult<Option<reth_primitives_traits::Bytecode>> {
        self.state()?.account_code(addr)
    }

    fn account_nonce(&self, addr: &Address) -> reth_provider::ProviderResult<Option<u64>> {
        self.state()?.account_nonce(addr)
    }

    fn account_balance(&self, addr: &Address) -> reth_provider::ProviderResult<Option<U256>> {
        self.state()?.account_balance(addr)
    }
}

impl<DB> AccountReader for RethDbWrapper<DB>
where
    DB: StateProviderFactory + Unpin + Clone + 'static
{
    fn basic_account(
        &self,
        address: &Address
    ) -> reth_provider::ProviderResult<Option<reth_primitives_traits::Account>> {
        self.state()?.basic_account(address)
    }
}

impl<DB> BlockHashReader for RethDbWrapper<DB>
where
    DB: StateProviderFactory + Unpin + Clone + 'static
{
    fn block_hash(&self, number: BlockNumber) -> reth_provider::ProviderResult<Option<B256>> {
        self.state()?.block_hash(number)
    }

    fn convert_block_hash(
        &self,
        hash_or_number: alloy::eips::BlockHashOrNumber
    ) -> reth_provider::ProviderResult<Option<B256>> {
        self.state()?.convert_block_hash(hash_or_number)
    }

    fn canonical_hashes_range(
        &self,
        start: BlockNumber,
        end: BlockNumber
    ) -> reth_provider::ProviderResult<Vec<B256>> {
        self.state()?.canonical_hashes_range(start, end)
    }
}

impl<DB> HashedPostStateProvider for RethDbWrapper<DB>
where
    DB: StateProviderFactory + Unpin + Clone + 'static
{
    fn hashed_post_state(&self, bundle_state: &BundleState) -> HashedPostState {
        self.state().unwrap().hashed_post_state(bundle_state)
    }
}

impl<DB> StateRootProvider for RethDbWrapper<DB>
where
    DB: StateProviderFactory + Unpin + Clone + 'static
{
    fn state_root(&self, hashed_state: HashedPostState) -> reth_provider::ProviderResult<B256> {
        self.state()?.state_root(hashed_state)
    }

    fn state_root_from_nodes(&self, input: TrieInput) -> reth_provider::ProviderResult<B256> {
        self.state()?.state_root_from_nodes(input)
    }

    fn state_root_with_updates(
        &self,
        hashed_state: HashedPostState
    ) -> reth_provider::ProviderResult<(B256, TrieUpdates)> {
        self.state()?.state_root_with_updates(hashed_state)
    }

    fn state_root_from_nodes_with_updates(
        &self,
        input: TrieInput
    ) -> reth_provider::ProviderResult<(B256, TrieUpdates)> {
        self.state()?.state_root_from_nodes_with_updates(input)
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
        self.state()?.storage_proof(address, slot, hashed_storage)
    }

    fn storage_root(
        &self,
        address: Address,
        hashed_storage: HashedStorage
    ) -> ProviderResult<B256> {
        self.state()?.storage_root(address, hashed_storage)
    }

    fn storage_multiproof(
        &self,
        address: Address,
        slots: &[B256],
        hashed_storage: HashedStorage
    ) -> ProviderResult<StorageMultiProof> {
        self.state()?
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
        self.state()?.proof(input, address, slots)
    }

    fn witness(&self, input: TrieInput, target: HashedPostState) -> ProviderResult<Vec<Bytes>> {
        self.state()?.witness(input, target)
    }

    fn multiproof(
        &self,
        input: TrieInput,
        targets: reth_trie::MultiProofTargets
    ) -> ProviderResult<MultiProof> {
        self.state()?.multiproof(input, targets)
    }
}

impl<DB> BytecodeReader for RethDbWrapper<DB>
where
    DB: StateProviderFactory + Unpin + Clone + 'static
{
    fn bytecode_by_hash(
        &self,
        code_hash: &B256
    ) -> reth_provider::ProviderResult<Option<reth_primitives_traits::Bytecode>> {
        self.state()?.bytecode_by_hash(code_hash)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering}
    };

    use alloy::eips::{BlockId, BlockNumberOrTag};
    use revm::DatabaseRef;

    use super::*;

    /// Records which block each read resolved against and refuses all of them,
    /// so a wrapper that returns a value is answering from something this
    /// factory never handed it.
    #[derive(Clone, Default)]
    struct RecordingFactory {
        resolved:     Arc<Mutex<Vec<BlockId>>>,
        latest_calls: Arc<AtomicUsize>
    }

    impl StateProviderFactory for RecordingFactory {
        fn latest(&self) -> ProviderResult<reth_provider::StateProviderBox> {
            self.latest_calls.fetch_add(1, Ordering::SeqCst);
            Err(ProviderError::BestBlockNotFound)
        }

        fn state_by_block_id(
            &self,
            block_id: BlockId
        ) -> ProviderResult<reth_provider::StateProviderBox> {
            self.resolved.lock().unwrap().push(block_id);
            Err(ProviderError::BestBlockNotFound)
        }

        fn state_by_block_number_or_tag(
            &self,
            _: BlockNumberOrTag
        ) -> ProviderResult<reth_provider::StateProviderBox> {
            unimplemented!()
        }

        fn history_by_block_number(
            &self,
            _: BlockNumber
        ) -> ProviderResult<reth_provider::StateProviderBox> {
            unimplemented!()
        }

        fn history_by_block_hash(
            &self,
            _: BlockHash
        ) -> ProviderResult<reth_provider::StateProviderBox> {
            unimplemented!()
        }

        fn state_by_block_hash(
            &self,
            _: BlockHash
        ) -> ProviderResult<reth_provider::StateProviderBox> {
            unimplemented!()
        }

        fn pending(&self) -> ProviderResult<reth_provider::StateProviderBox> {
            unimplemented!()
        }

        fn pending_state_by_hash(
            &self,
            _: B256
        ) -> ProviderResult<Option<reth_provider::StateProviderBox>> {
            unimplemented!()
        }

        fn maybe_pending(&self) -> ProviderResult<Option<reth_provider::StateProviderBox>> {
            unimplemented!()
        }
    }

    impl BlockNumReader for RecordingFactory {
        fn chain_info(&self) -> ProviderResult<ChainInfo> {
            unimplemented!()
        }

        fn best_block_number(&self) -> ProviderResult<BlockNumber> {
            unimplemented!()
        }

        fn last_block_number(&self) -> ProviderResult<BlockNumber> {
            unimplemented!()
        }

        fn block_number(&self, _: B256) -> ProviderResult<Option<BlockNumber>> {
            unimplemented!()
        }
    }

    impl BlockHashReader for RecordingFactory {
        /// Absent rather than erroring, so `block_hash_ref` is exercised on the
        /// case that used to default to the zero hash.
        fn block_hash(&self, _: BlockNumber) -> ProviderResult<Option<B256>> {
            Ok(None)
        }

        fn canonical_hashes_range(
            &self,
            _: BlockNumber,
            _: BlockNumber
        ) -> ProviderResult<Vec<B256>> {
            unimplemented!()
        }
    }

    impl BlockIdReader for RecordingFactory {
        fn pending_block_num_hash(&self) -> ProviderResult<Option<BlockNumHash>> {
            unimplemented!()
        }

        fn safe_block_num_hash(&self) -> ProviderResult<Option<BlockNumHash>> {
            unimplemented!()
        }

        fn finalized_block_num_hash(&self) -> ProviderResult<Option<BlockNumHash>> {
            unimplemented!()
        }
    }

    const PARENT: BlockNumHash = BlockNumHash { number: 42, hash: B256::repeat_byte(0x11) };

    fn wrapper() -> (RecordingFactory, RethDbWrapper<RecordingFactory>) {
        let factory = RecordingFactory::default();
        (factory.clone(), RethDbWrapper::new(factory, PARENT))
    }

    #[test]
    fn every_read_resolves_against_the_selected_block() {
        let (factory, wrapper) = wrapper();

        let _ = wrapper.basic_ref(Address::ZERO);
        let _ = wrapper.storage_ref(Address::ZERO, U256::ZERO);
        let _ = wrapper.bytecode_by_hash(&B256::repeat_byte(0xcd));
        let _ = wrapper.state_root(HashedPostState::default());
        let _ = wrapper.block_hash_ref(7);

        // The selector names a branch, not a height, so a same-height reorg is
        // expressible. Under the old `AtomicU64` this was a bare number.
        let resolved = factory.resolved.lock().unwrap().clone();
        assert_eq!(resolved.len(), 4);
        assert!(resolved.iter().all(|id| *id == BlockId::from(PARENT.hash)), "{resolved:?}");

        // The tip is never consulted, so no read can fall back to current state.
        assert_eq!(factory.latest_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn unavailable_state_errors_rather_than_reading_as_zero() {
        let (_, wrapper) = wrapper();

        // Each of these used to hand back a value indistinguishable from a real
        // one: an empty account, a zero slot, empty bytecode.
        assert!(wrapper.basic_ref(Address::ZERO).is_err());
        assert!(wrapper.storage_ref(Address::ZERO, U256::ZERO).is_err());
        assert!(wrapper.code_by_hash_ref(B256::repeat_byte(0xcd)).is_err());
    }

    #[test]
    fn an_unknown_block_hash_is_an_error_not_the_zero_hash() {
        let (_, wrapper) = wrapper();

        // The factory resolves the lookup and reports no such block, so this is
        // the absent-value case rather than the unavailable-state one.
        assert!(wrapper.block_hash_ref(7).is_err());
    }

    #[test]
    fn empty_code_resolves_without_touching_state() {
        let (factory, wrapper) = wrapper();

        // An account with no code is not a failed read, and answering it must not
        // depend on state being available.
        assert_eq!(wrapper.code_by_hash_ref(KECCAK_EMPTY).unwrap(), Bytecode::default());
        assert!(factory.resolved.lock().unwrap().is_empty());
    }

    #[test]
    fn a_view_of_another_block_leaves_this_one_alone() {
        let (factory, wrapper) = wrapper();
        let other_branch = BlockNumHash { number: 42, hash: B256::repeat_byte(0x22) };

        // Same height, different branch: a second view, not a moved selector.
        let other = wrapper.at_block(other_branch);
        assert_eq!(wrapper.block(), PARENT);
        assert_eq!(other.block(), other_branch);

        let _ = wrapper.storage_ref(Address::ZERO, U256::ZERO);
        let _ = other.storage_ref(Address::ZERO, U256::ZERO);
        assert_eq!(
            *factory.resolved.lock().unwrap(),
            vec![BlockId::from(PARENT.hash), BlockId::from(other_branch.hash)]
        );
    }

    // ==== TEMPORARY PROOF TEST (issue 1b) ====

    /// A node whose *startup* block state is gone (pruned / reorged out) but
    /// whose chain still knows every block hash and every other block-number
    /// lookup.
    #[derive(Clone, Default)]
    struct PrunedStartupFactory {
        resolved: Arc<Mutex<Vec<BlockId>>>
    }

    impl PrunedStartupFactory {
        fn hash_of(n: BlockNumber) -> B256 {
            B256::from(U256::from(n))
        }
    }

    impl StateProviderFactory for PrunedStartupFactory {
        fn state_by_block_id(&self, block_id: BlockId) -> ProviderResult<reth_provider::StateProviderBox> {
            self.resolved.lock().unwrap().push(block_id);
            // Only the startup block's state is unavailable.
            if block_id == BlockId::from(PARENT.hash) {
                return Err(ProviderError::StateForHashNotFound(PARENT.hash));
            }
            unimplemented!("state for other blocks is available in this scenario")
        }
        fn latest(&self) -> ProviderResult<reth_provider::StateProviderBox> { unimplemented!() }
        fn state_by_block_number_or_tag(&self, _: BlockNumberOrTag) -> ProviderResult<reth_provider::StateProviderBox> { unimplemented!() }
        fn history_by_block_number(&self, _: BlockNumber) -> ProviderResult<reth_provider::StateProviderBox> { unimplemented!() }
        fn history_by_block_hash(&self, _: BlockHash) -> ProviderResult<reth_provider::StateProviderBox> { unimplemented!() }
        fn state_by_block_hash(&self, _: BlockHash) -> ProviderResult<reth_provider::StateProviderBox> { unimplemented!() }
        fn pending(&self) -> ProviderResult<reth_provider::StateProviderBox> { unimplemented!() }
        fn pending_state_by_hash(&self, _: B256) -> ProviderResult<Option<reth_provider::StateProviderBox>> { unimplemented!() }
        fn maybe_pending(&self) -> ProviderResult<Option<reth_provider::StateProviderBox>> { unimplemented!() }
    }

    impl BlockHashReader for PrunedStartupFactory {
        /// The canonical chain still knows every hash.
        fn block_hash(&self, number: BlockNumber) -> ProviderResult<Option<B256>> {
            Ok(Some(Self::hash_of(number)))
        }
        fn canonical_hashes_range(&self, _: BlockNumber, _: BlockNumber) -> ProviderResult<Vec<B256>> { unimplemented!() }
    }

    impl BlockNumReader for PrunedStartupFactory {
        fn chain_info(&self) -> ProviderResult<ChainInfo> { unimplemented!() }
        fn best_block_number(&self) -> ProviderResult<BlockNumber> { Ok(PARENT.number + 10) }
        fn last_block_number(&self) -> ProviderResult<BlockNumber> { Ok(PARENT.number + 10) }
        fn block_number(&self, _: B256) -> ProviderResult<Option<BlockNumber>> { Ok(Some(PARENT.number + 1)) }
        fn convert_number(&self, _: alloy::eips::BlockHashOrNumber) -> ProviderResult<Option<B256>> {
            Ok(Some(Self::hash_of(PARENT.number + 1)))
        }
    }

    impl BlockIdReader for PrunedStartupFactory {
        fn pending_block_num_hash(&self) -> ProviderResult<Option<BlockNumHash>> { unimplemented!() }
        fn safe_block_num_hash(&self) -> ProviderResult<Option<BlockNumHash>> { unimplemented!() }
        fn finalized_block_num_hash(&self) -> ProviderResult<Option<BlockNumHash>> { unimplemented!() }
    }

    #[test]
    fn proof_b_new_head_hash_lookup_needs_the_pinned_blocks_state() {
        let factory = PrunedStartupFactory::default();
        let wrapper = RethDbWrapper::new(factory.clone(), PARENT);
        let new_head = PARENT.number + 1;

        // This is exactly the call `Validator::head_view` makes:
        //   self.db.block_hash(number)   (BlockHashReader, via BlockNumReader's supertrait)
        let via_reader = BlockHashReader::block_hash(&wrapper, new_head);
        println!("PROOF-B BlockHashReader::block_hash({new_head}) -> {via_reader:?}");

        // The unpinned siblings answer the same question fine.
        let via_db_ref = wrapper.block_hash_ref(new_head);
        println!("PROOF-B DatabaseRef::block_hash_ref({new_head}) -> {via_db_ref:?}");
        let via_convert = BlockNumReader::convert_number(&wrapper, new_head.into());
        println!("PROOF-B BlockNumReader::convert_number({new_head}) -> {via_convert:?}");

        let resolved = factory.resolved.lock().unwrap().clone();
        println!("PROOF-B state_by_block_id calls: {resolved:?}  (pinned = {:?})", PARENT.hash);

        assert!(via_reader.is_err(), "head-hash lookup went through the pinned state");
        assert_eq!(via_db_ref.unwrap(), PrunedStartupFactory::hash_of(new_head));
        assert_eq!(via_convert.unwrap(), Some(PrunedStartupFactory::hash_of(new_head)));
        assert_eq!(resolved, vec![BlockId::from(PARENT.hash)]);
    }

}
