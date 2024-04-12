use std::sync::Arc;

use alloy_primitives::{hex, Address, U256};
use reth_primitives::keccak256;
use reth_provider::StateProviderFactory;
use revm::DatabaseRef;

use super::ANGSTROM_CONTRACT;
use crate::order::state::RevmLRU;

/// The nonce location for quick db lookup
const ANGSTROM_NONCE_SLOT_CONST: [u8; 4] = hex!("daa050e9");

pub struct Nonces;

impl Nonces {
    pub fn is_valid_nonce<DB: StateProviderFactory>(
        &self,
        user: Address,
        nonce: u64,
        db: Arc<RevmLRU<DB>>
    ) -> bool {
        let nonce = nonce.to_be_bytes();
        let mut arry = [0u8; 31];
        arry[0..20].copy_from_slice(&**user);
        arry[20..24].copy_from_slice(&ANGSTROM_NONCE_SLOT_CONST);
        arry[24..31].copy_from_slice(&nonce[0..7]);
        let slot = keccak256(arry);

        let word = db.storage_ref(ANGSTROM_CONTRACT, slot.into()).unwrap();
        tracing::debug!(?world);
        let mut flag = U256::from(1) << nonce[7];

        (word ^ flag) & flag == flag
    }
}
