use std::{fmt::Debug, sync::Arc};

use alloy::primitives::{Address, B256, U256, hex, keccak256};
use reth_revm::DatabaseRef;

/// The nonce location for quick db lookup
const ANGSTROM_NONCE_SLOT_CONST: [u8; 4] = hex!("daa050e9");

#[derive(Clone)]
pub struct Nonces(Address);

impl Nonces {
    pub fn new(angstrom_address: Address) -> Self {
        Self(angstrom_address)
    }

    pub fn get_nonce_word_slot(&self, user: Address, nonce: u64) -> B256 {
        let nonce = nonce.to_be_bytes();
        let mut arry = [0u8; 31];
        arry[0..20].copy_from_slice(&**user);
        arry[20..24].copy_from_slice(&ANGSTROM_NONCE_SLOT_CONST);
        arry[24..31].copy_from_slice(&nonce[0..7]);
        keccak256(arry)
    }

    pub fn is_valid_nonce<DB: revm::DatabaseRef>(
        &self,
        user: Address,
        nonce: u64,
        db: Arc<DB>
    ) -> Result<bool, DB::Error>
    where
        <DB as DatabaseRef>::Error: Sync + Send + 'static + Debug
    {
        let slot = self.get_nonce_word_slot(user, nonce);

        // Unavailable state (a view of a block a reorg removed) is an error for the
        // caller, not a panic: this runs inside validation's thread pool, where a
        // panic takes the whole validator down.
        let word = db.storage_ref(self.0, slot.into())?;
        tracing::debug!(?word);
        let flag = U256::from(1) << (nonce as u8);

        let out = (word ^ flag) & flag == flag;
        tracing::debug!(?word, %out);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use alloy::primitives::{Address, B256, U256};
    use angstrom_types::reth_db_wrapper::DBError;
    use reth_provider::ProviderError;
    use revm::{bytecode::Bytecode, state::AccountInfo};

    use crate::order::state::db_state_utils::{FetchUtils, StateFetchUtils};

    /// A view of a block a reorg removed: every read fails the way
    /// `RethDbWrapper`'s does once the hash has left the canonical chain.
    #[derive(Clone)]
    struct Orphaned;

    fn orphaned() -> DBError {
        ProviderError::BlockHashNotFound(B256::repeat_byte(0x0d)).into()
    }

    impl revm::DatabaseRef for Orphaned {
        type Error = DBError;

        fn basic_ref(&self, _: Address) -> Result<Option<AccountInfo>, DBError> {
            Err(orphaned())
        }

        fn code_by_hash_ref(&self, _: B256) -> Result<Bytecode, DBError> {
            Err(orphaned())
        }

        fn storage_ref(&self, _: Address, _: U256) -> Result<U256, DBError> {
            Err(orphaned())
        }

        fn block_hash_ref(&self, _: u64) -> Result<B256, DBError> {
            Err(orphaned())
        }
    }

    #[test]
    fn a_nonce_read_on_an_unavailable_view_is_an_error_not_a_panic() {
        let fetch = FetchUtils::new(Address::repeat_byte(0xaa), Arc::new(Orphaned));

        let error = fetch
            .is_valid_nonce(Address::repeat_byte(0x11), 0)
            .unwrap_err();
        assert!(error.to_string().contains("BlockHashNotFound"), "{error}");
    }
}
