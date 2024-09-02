//! keeps track of account state for orders
use std::{collections::HashSet, sync::Arc};

use alloy_primitives::B256;
use angstrom_types::sol_bindings::grouped_orders::{OrderWithStorageData, RawPoolOrder};
use dashmap::DashSet;
use parking_lot::RwLock;
use thiserror::Error;
use user::UserAccounts;

use super::db_state_utils::FetchUtils;
use crate::{common::lru_db::BlockStateProviderFactory, RevmLRU};

pub mod user;

/// processes a user account and tells us based on there current live orders
/// wether or not this order is valid.
pub struct UserAccountProcessor<DB> {
    /// database for fetching verification info
    db:                    Arc<RevmLRU<DB>>,
    /// keeps track of all user accounts
    user_accounts:         UserAccounts,
    /// utils for fetching the required data to verify
    /// a order.
    fetch_utils:           FetchUtils,
    /// to ensure that we don't re-validate a canceled order
    known_canceled_orders: DashSet<B256>
}

impl<DB: BlockStateProviderFactory + Unpin + 'static> UserAccountProcessor<DB> {
    /// Fetches the state overrides that are required for the hook simulation.
    pub fn grab_state_for_hook_simulations<O: RawPoolOrder>(
        &mut self,
        order: O,
        block: u64
    ) -> Result<(), UserAccountVerificationError> {
        Ok(())
    }

    pub fn verify_order<O: RawPoolOrder>(
        &mut self,
        order: O,
        block: u64
    ) -> Result<OrderWithStorageData<O>, UserAccountVerificationError> {
        let current_block = self.user_accounts.current_block();
        if block != current_block {
            return Err(UserAccountVerificationError::BlockMissMatch {
                requested: block,
                current:   current_block
            })
        }

        let order_hash = order.hash();
        if self.known_canceled_orders.contains(&order_hash) {
            return Err(UserAccountVerificationError::OrderIsCancelled(order_hash))
        }

        todo!()
    }
}

#[derive(Debug, Error)]
pub enum UserAccountVerificationError {
    #[error("tried to verify for block {} where current is {}", requested, current)]
    BlockMissMatch { requested: u64, current: u64 },
    #[error("order hash has been cancelled {0:?}")]
    OrderIsCancelled(B256)
}
