use std::{collections::HashMap, sync::Arc};

use alloy_primitives::{keccak256, Address, B256, U256};
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

    pub fn fetch_approval_balance_for_token<DB: StateProviderFactory>(
        &self,
        user: Address,
        token: Address,
        db: Arc<RevmLRU<DB>>
    ) -> Option<U256> {
        let slot_i: U256 = self.0.get(&token)?.clone();
        let mut slot = user.to_vec();
        slot.extend(slot_i.to_be_bytes::<32>().to_vec());
        let slot_addr = keccak256(slot);

        db.storage_ref(token, U256::from_be_bytes(*slot_addr)).ok()
    }
}
