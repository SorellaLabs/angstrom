use std::{collections::HashMap, sync::Arc};

use alloy_primitives::{Address, U256};
use reth_revm::DatabaseRef;

use crate::{
    common::lru_db::{BlockStateProviderFactory, RevmLRU},
    order::state::config::TokenBalanceSlot
};

#[derive(Clone)]
pub struct Balances(HashMap<Address, TokenBalanceSlot>);

impl Balances {
    pub fn new(slots: HashMap<Address, TokenBalanceSlot>) -> Self {
        Self(slots)
    }

    pub fn fetch_balance_for_token_overrides<DB: BlockStateProviderFactory>(
        &self,
        user: Address,
        token: Address,
        db: Arc<RevmLRU<DB>>,
        overrides: &HashMap<Address, HashMap<U256, U256>>
    ) -> Option<U256> {
        self.0.get(&token).and_then(|slot| {
            let slot_addr = slot.generate_slot(user).ok()?;
            if let Some(address_slots) = overrides.get(&token) {
                if let Some(s_override) = address_slots.get(&slot_addr) {
                    return Some(*s_override)
                }
            }
            db.storage_ref(token, slot_addr).ok()
        })
    }

    pub fn fetch_balance_for_token<DB: BlockStateProviderFactory>(
        &self,
        user: Address,
        token: Address,
        db: Arc<RevmLRU<DB>>
    ) -> Option<U256> {
        self.0
            .get(&token)
            .and_then(|slot| slot.load_balance(user, db).ok())
    }
}
