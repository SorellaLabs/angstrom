use std::{collections::HashMap, fmt::Debug, sync::Arc};

use alloy::primitives::{Address, U256};
use dashmap::DashMap;
use reth_revm::DatabaseRef;

use super::finders::find_slot_offset_for_approval;
use crate::order::state::config::TokenApprovalSlot;

#[derive(Clone)]
pub struct Approvals {
    angstrom_address: Address,
    slots:            DashMap<Address, TokenApprovalSlot>
}

impl Approvals {
    pub fn new(angstrom_address: Address) -> Self {
        Self { angstrom_address, slots: DashMap::default() }
    }

    pub fn fetch_approval_balance_for_token_overrides<DB: revm::DatabaseRef>(
        &self,
        user: Address,
        token: Address,
        db: Arc<DB>,
        overrides: &HashMap<Address, HashMap<U256, U256>>
    ) -> Option<U256>
    where
        <DB as revm::DatabaseRef>::Error: Debug
    {
        self.slots
            .get(&token)
            .or_else(|| {
                let slot = find_slot_offset_for_approval(&db, token);
                let slot = TokenApprovalSlot::new(token, slot as u8);
                self.slots.insert(token, slot);
                self.slots.get(&token)
            })
            .and_then(|slot| {
                let slot_addr = slot.generate_slot(user, self.angstrom_address).ok()?;
                if let Some(address_slots) = overrides.get(&token) {
                    if let Some(s_override) = address_slots.get(&slot_addr) {
                        return Some(*s_override)
                    }
                }

                db.storage_ref(token, slot_addr).ok()
            })
    }

    pub fn fetch_approval_balance_for_token<DB: revm::DatabaseRef>(
        &self,
        user: Address,
        token: Address,
        db: &DB
    ) -> Option<U256>
    where
        <DB as DatabaseRef>::Error: Debug + Sync + Send + 'static
    {
        self.slots
            .get(&token)
            .or_else(|| {
                let slot = find_slot_offset_for_approval(db, token);
                let slot = TokenApprovalSlot::new(token, slot as u8);
                self.slots.insert(token, slot);
                self.slots.get(&token)
            })
            .and_then(|slot| {
                slot.load_approval_amount(user, self.angstrom_address, db)
                    .ok()
            })
    }
}
