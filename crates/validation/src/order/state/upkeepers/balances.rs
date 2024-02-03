use std::{collections::HashMap, sync::Arc};

use alloy_primitives::{keccak256, Address, FixedBytes, B256, U256};
use alloy_sol_macro::sol;
use parking_lot::RwLock;
use reth_provider::{StateProvider, StateProviderFactory};
use revm::DatabaseRef;

use crate::common::lru_db::RevmLRU;

sol!( function balanceOf( address who ) public view returns (uint value););

#[derive(Clone)]
pub struct Balances(HashMap<Address, U256>);

impl Balances {
    pub fn new(slots: HashMap<Address, U256>) -> Self {
        Self(slots)
    }

    fn get_slot_address(&self, user: Address, token: Address) -> Option<FixedBytes<32>> {
        let slot_i: U256 = self.0.get(&token)?.clone();
        let mut slot = user.to_vec();
        slot.extend(slot_i.to_be_bytes::<32>().to_vec());
        Some(keccak256(slot))
    }

    pub fn fetch_balance_for_token_overrides<DB: StateProviderFactory>(
        &self,
        user: Address,
        token: Address,
        db: Arc<RevmLRU<DB>>,
        overrides: &HashMap<Address, HashMap<U256, U256>>
    ) -> Option<U256> {
        if let Some(address_slots) = overrides.get(&token) {
            let slot_addr = U256::from_be_bytes(*self.get_slot_address(token, user)?);
            return address_slots
                .get(&slot_addr)
                .map(|slot| Some(*slot))
                .unwrap_or_else(|| self.fetch_balance_for_token(user, token, db))
        }

        self.fetch_balance_for_token(user, token, db)
    }

    pub fn fetch_balance_for_token<DB: StateProviderFactory>(
        &self,
        user: Address,
        token: Address,
        db: Arc<RevmLRU<DB>>
    ) -> Option<U256> {
        let slot_addr = self.get_slot_address(user, token)?;

        db.storage_ref(token, U256::from_be_bytes(*slot_addr)).ok()
    }
}
