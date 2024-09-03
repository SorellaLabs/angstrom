//! keeps track of account state for orders
use std::{
    collections::HashSet,
    sync::{atomic::AtomicU64, Arc}
};

use alloy_primitives::B256;
use angstrom_types::sol_bindings::grouped_orders::{OrderWithStorageData, PoolOrder, RawPoolOrder};
use dashmap::DashSet;
use parking_lot::RwLock;
use thiserror::Error;
use user::UserAccounts;

use super::{
    db_state_utils::FetchUtils,
    pools::{index_to_address::AssetIndexToAddressWrapper, UserOrderPoolInfo},
    ValidationConfig
};
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
    pub fn new(db: Arc<RevmLRU<DB>>, config: ValidationConfig, current_block: u64) -> Self {
        let user_accounts = UserAccounts::new(current_block);
        let fetch_utils = FetchUtils::new(config);
        Self { db, fetch_utils, user_accounts, known_canceled_orders: DashSet::default() }
    }

    /// Fetches the state overrides that are required for the hook simulation.
    pub fn grab_state_for_hook_simulations<O: RawPoolOrder>(
        &self,
        order: AssetIndexToAddressWrapper<O>,
        pool_info: UserOrderPoolInfo,
        block: u64
    ) -> Result<(), UserAccountVerificationError<O>> {
        Ok(())
    }

    pub fn verify_order<O: RawPoolOrder>(
        &self,
        order: AssetIndexToAddressWrapper<O>,
        pool_info: UserOrderPoolInfo,
        block: u64,
        is_limit: bool
    ) -> Result<OrderWithStorageData<O>, UserAccountVerificationError<O>> {
        let current_block = self.user_accounts.current_block();
        // ensure baseline data for block is up to date
        if block != current_block {
            return Err(UserAccountVerificationError::BlockMissMatch {
                requested: block,
                current: current_block,
                order,
                pool_info
            })
        }

        // see if order has been cancelled before
        let order_hash = order.hash();
        if self.known_canceled_orders.contains(&order_hash) {
            return Err(UserAccountVerificationError::OrderIsCancelled(order_hash))
        }

        let user = order.from();
        let nonce = order.nonce();
        // validate we don't have a nonce conflict.
        if self.user_accounts.has_nonce_conflict(user, nonce) {
            return Err(UserAccountVerificationError::DuplicateNonce(order_hash))
        }

        let live_state = self.user_accounts.get_live_state_for_order(
            user,
            pool_info.token,
            nonce,
            &self.fetch_utils,
            &self.db
        );

        // ensure that the current live state is enough to satisfy the order
        let (is_cur_valid, invalid_orders) =
            if let Some(pending_user_action) = live_state.can_support_order(&order, &pool_info) {
                // we can satisfy this order. lets check the higher level nonces to see if we
                // invalidate anything
                let invalid_orders = self
                    .user_accounts
                    .insert_pending_user_action(order.from(), pending_user_action);
                (true, invalid_orders)
            } else {
                (false, vec![])
            };

        Ok(order.into_order_storage_with_data(
            block,
            is_cur_valid,
            true,
            is_limit,
            pool_info,
            invalid_orders
        ))
    }
}

#[derive(Debug, Error)]
pub enum UserAccountVerificationError<O: RawPoolOrder> {
    #[error("tried to verify for block {} where current is {}", requested, current)]
    BlockMissMatch {
        requested: u64,
        current:   u64,
        order:     AssetIndexToAddressWrapper<O>,
        pool_info: UserOrderPoolInfo
    },
    #[error("order hash has been cancelled {0:?}")]
    OrderIsCancelled(B256),
    #[error("Nonce exists for a current order hash: {0:?}")]
    DuplicateNonce(B256)
}
