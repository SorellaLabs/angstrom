use std::{collections::HashMap, sync::Arc};

use alloy::primitives::{Address, U256};
use reth_revm::DatabaseRef;

use crate::order::state::config::TokenBalanceSlot;

#[derive(Clone)]
pub struct Balances(HashMap<Address, TokenBalanceSlot>);

impl Balances {
    pub fn new(slots: HashMap<Address, TokenBalanceSlot>) -> Self {
        Self(slots)
    }

    pub fn fetch_balance_for_token_overrides<DB: revm::DatabaseRef>(
        &self,
        user: Address,
        token: Address,
        db: Arc<DB>,
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

    pub fn fetch_balance_for_token<DB: revm::DatabaseRef>(
        &self,
        user: Address,
        token: Address,
        db: &DB
    ) -> Option<U256>
    where
        <DB as DatabaseRef>::Error: Sync + Send + 'static
    {
        self.0
            .get(&token)
            .and_then(|slot| slot.load_balance(user, db).ok())
    }
}
