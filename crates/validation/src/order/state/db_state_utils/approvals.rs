use std::{collections::HashMap, sync::Arc};

use alloy::primitives::{Address, U256};
use reth_revm::DatabaseRef;

use crate::order::state::config::TokenApprovalSlot;

#[derive(Clone)]
pub struct Approvals {
    angstrom_address: Address,
    slots:            HashMap<Address, TokenApprovalSlot>
}

impl Approvals {
    pub fn new(angstrom_address: Address, slots: HashMap<Address, TokenApprovalSlot>) -> Self {
        Self { angstrom_address, slots }
    }

    pub fn fetch_approval_balance_for_token_overrides<DB: revm::DatabaseRef>(
        &self,
        user: Address,
        token: Address,
        db: Arc<DB>,
        overrides: &HashMap<Address, HashMap<U256, U256>>
    ) -> Option<U256> {
        self.slots.get(&token).and_then(|slot| {
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
    ) -> Option<U256> {
        self.slots.get(&token).and_then(|slot| {
            slot.load_approval_amount(user, self.angstrom_address, db)
                .ok()
        })
    }
}
