use std::{collections::HashMap, fmt::Debug, sync::Arc};

use alloy::{
    primitives::{keccak256, Address, U256},
    sol_types::SolValue
};
use dashmap::DashMap;
use reth_revm::DatabaseRef;

use super::finders::find_slot_offset_for_balance;
use crate::order::state::config::TokenBalanceSlot;

#[derive(Clone)]
pub struct Balances {
    tokens:           DashMap<Address, TokenBalanceSlot>,
    angstrom_address: Address
}
const ANGSTROM_BALANCE_SLOT_OFFSET: u32 = 4;

impl Balances {
    pub fn new(angstrom_address: Address) -> Self {
        Self { tokens: DashMap::default(), angstrom_address }
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
        // Existing code remains unchanged
        self.tokens
            .get(&token)
            .or_else(|| {
                let slot = find_slot_offset_for_balance(&db, token);
                let slot = TokenBalanceSlot::new(token, slot as u8);
                self.tokens.insert(token, slot);
                self.tokens.get(&token)
            })
            .and_then(|slot| {
                let slot_addr = slot.generate_slot(user).ok()?;
                if let Some(address_slots) = overrides.get(&token) {
                    if let Some(s_override) = address_slots.get(&slot_addr) {
                        return Some(*s_override);
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
    ) -> U256
    where
        <DB as DatabaseRef>::Error: Debug + Sync + Send + 'static
    {
        self.tokens
            .get(&token)
            .or_else(|| {
                let slot = find_slot_offset_for_balance(db, token);
                let slot = TokenBalanceSlot::new(token, slot as u8);
                self.tokens.insert(token, slot);
                self.tokens.get(&token)
            })
            .and_then(|slot| slot.load_balance(user, db).ok())
            .unwrap_or_default()
    }

    pub fn fetch_balance_in_angstrom<DB: revm::DatabaseRef>(
        &self,
        token: Address,
        account: Address,
        db: &DB
    ) -> U256 {
        let token_slot = keccak256((token, ANGSTROM_BALANCE_SLOT_OFFSET).abi_encode());
        let final_slot = keccak256((token_slot, account).abi_encode());
        db.storage_ref(self.angstrom_address, U256::from_be_bytes(*final_slot.as_ref()))
            .unwrap_or_default()
    }
}
