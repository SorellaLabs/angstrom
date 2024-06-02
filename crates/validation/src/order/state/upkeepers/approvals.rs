use std::{collections::HashMap, sync::Arc};

use alloy_primitives::{keccak256, Address, FixedBytes, B256, U256};
use alloy_sol_macro::sol;
use parking_lot::RwLock;
use reth_provider::StateProvider;
use reth_revm::DatabaseRef;

use super::ANGSTROM_CONTRACT;
use crate::order::state::{config::TokenApprovalSlot, BlockStateProviderFactory, RevmLRU};

#[derive(Clone)]
pub struct Approvals(HashMap<Address, TokenApprovalSlot>);

impl Approvals {
    pub fn new(current_slots: HashMap<Address, TokenApprovalSlot>) -> Self {
        Self(current_slots)
    }

    pub fn fetch_approval_balance_for_token_overrides<DB: BlockStateProviderFactory>(
        &self,
        user: Address,
        token: Address,
        db: Arc<RevmLRU<DB>>,
        overrides: &HashMap<Address, HashMap<U256, U256>>
    ) -> Option<U256> {
        self.0.get(&token).and_then(|slot| {
            let slot_addr = slot.generate_slot(user, ANGSTROM_CONTRACT).ok()?;
            if let Some(address_slots) = overrides.get(&token) {
                if let Some(s_override) = address_slots.get(&slot_addr) {
                    return Some(*s_override)
                }
            }

            db.storage_ref(token, slot_addr).ok()
        })
    }

    pub fn fetch_approval_balance_for_token<DB: BlockStateProviderFactory>(
        &self,
        user: Address,
        token: Address,
        db: Arc<RevmLRU<DB>>
    ) -> Option<U256> {
        self.0
            .get(&token)
            .and_then(|slot| slot.load_approval_amount(user, ANGSTROM_CONTRACT, db).ok())
    }
}
