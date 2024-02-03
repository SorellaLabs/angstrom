use std::{collections::HashMap, sync::Arc};

use alloy_primitives::{keccak256, Address, FixedBytes, B256, U256};
use alloy_sol_macro::sol;
use parking_lot::RwLock;
use reth_provider::{StateProvider, StateProviderFactory};
use reth_revm::DatabaseRef;

use crate::order::state::RevmLRU;

sol! (
    function allowance( address owner, address spender ) public view returns (uint _allowance);
);

#[derive(Clone)]
pub struct Approvals(HashMap<Address, U256>);

impl Approvals {
    pub fn new(current_slots: HashMap<Address, U256>) -> Self {
        Self(current_slots)
    }

    pub fn fetch_approval_balance_for_token_overrides<DB: StateProviderFactory>(
        &self,
        user: Address,
        token: Address,
        db: Arc<RevmLRU<DB>>,
        overrides: &HashMap<Address, HashMap<U256, U256>>
    ) -> Option<U256> {
        if let Some(address_slots) = overrides.get(&token) {
            let slot_addr = U256::from_be_bytes(*self.get_slot(token, user)?);
            return address_slots
                .get(&slot_addr)
                .map(|slot| Some(*slot))
                .unwrap_or_else(|| self.fetch_approval_balance_for_token(user, token, db))
        }

        self.fetch_approval_balance_for_token(user, token, db)
    }

    fn get_slot(&self, token: Address, user: Address) -> Option<FixedBytes<32>> {
        let slot_i: U256 = self.0.get(&token)?.clone();
        let mut slot = user.to_vec();
        slot.extend(slot_i.to_be_bytes::<32>().to_vec());
        Some(keccak256(slot))
    }

    pub fn fetch_approval_balance_for_token<DB: StateProviderFactory>(
        &self,
        user: Address,
        token: Address,
        db: Arc<RevmLRU<DB>>
    ) -> Option<U256> {
        let slot_addr = self.get_slot(token, user)?;
        db.storage_ref(token, U256::from_be_bytes(*slot_addr)).ok()
    }
}
