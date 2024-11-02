use std::{collections::HashMap, fmt::Debug, sync::Arc};

use alloy::primitives::{Address, U256};
use dashmap::DashMap;
use reth_revm::DatabaseRef;

use super::finders::find_slot_offset_for_balance;
use crate::order::state::config::TokenBalanceSlot;

#[derive(Clone)]
pub struct Balances(DashMap<Address, TokenBalanceSlot>);

impl Default for Balances {
    fn default() -> Self {
        Self::new()
    }
}

impl Balances {
    pub fn new() -> Self {
        Self(DashMap::default())
    }

    pub fn fetch_balance_for_token_overrides<DB: revm::DatabaseRef>(
        &self,
        user: Address,
        token: Address,
        db: Arc<DB>,
        overrides: &HashMap<Address, HashMap<U256, U256>>
    ) -> Option<U256>
    where
        <DB as revm::DatabaseRef>::Error: Debug
    {
        self.0
            .get(&token)
            .or_else(|| {
                let slot = find_slot_offset_for_balance(&db, token);
                let slot = TokenBalanceSlot::new(token, slot as u8);
                self.0.insert(token, slot);
                self.0.get(&token)
            })
            .and_then(|slot| {
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
        <DB as DatabaseRef>::Error: Debug + Sync + Send + 'static
    {
        self.0
            .get(&token)
            .or_else(|| {
                let slot = find_slot_offset_for_balance(db, token);
                let slot = TokenBalanceSlot::new(token, slot as u8);
                self.0.insert(token, slot);
                self.0.get(&token)
            })
            .and_then(|slot| slot.load_balance(user, db).ok())
    }
}
