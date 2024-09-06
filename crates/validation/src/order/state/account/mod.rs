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
    db_state_utils::{FetchUtils, StateFetchUtils},
    pools::{index_to_address::AssetIndexToAddressWrapper, UserOrderPoolInfo},
    ValidationConfig
};
use crate::{common::lru_db::BlockStateProviderFactory, RevmLRU};

pub mod user;

/// processes a user account and tells us based on there current live orders
/// wether or not this order is valid.
pub struct UserAccountProcessor<DB, S> {
    /// database for fetching verification info
    db:                    Arc<RevmLRU<DB>>,
    /// keeps track of all user accounts
    user_accounts:         UserAccounts,
    /// utils for fetching the required data to verify
    /// a order.
    fetch_utils:           S,
    /// to ensure that we don't re-validate a canceled order
    known_canceled_orders: DashSet<B256>
}

impl<DB: BlockStateProviderFactory + Unpin + 'static, S: StateFetchUtils>
    UserAccountProcessor<DB, S>
{
    pub fn new(
        db: Arc<RevmLRU<DB>>,
        config: ValidationConfig,
        current_block: u64,
        fetch_utils: S
    ) -> Self {
        let user_accounts = UserAccounts::new(current_block);
        // let fetch_utils = FetchUtils::new(config);
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
        let nonce = order.nonce();
        let user = order.from();
        let order_hash = order.hash();

        // very nonce hasn't been used historically
        if !self
            .fetch_utils
            .is_valid_nonce(user, nonce.to(), self.db.clone())
        {
            return Err(UserAccountVerificationError::DuplicateNonce(order_hash))
        }

        // very we don't have a pending nonce conflict
        if self.user_accounts.has_nonce_conflict(user, nonce) {
            return Err(UserAccountVerificationError::DuplicateNonce(order_hash))
        }

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
        if self.known_canceled_orders.contains(&order_hash) {
            return Err(UserAccountVerificationError::OrderIsCancelled(order_hash))
        }

        let live_state = self.user_accounts.get_live_state_for_order(
            user,
            pool_info.token,
            nonce,
            &self.fetch_utils,
            &self.db
        );

        // ensure that the current live state is enough to satisfy the order
        let (is_cur_valid, invalid_orders) = live_state
            .can_support_order(&order, &pool_info)
            .map(|pending_user_action| {
                (
                    true,
                    self.user_accounts
                        .insert_pending_user_action(order.from(), pending_user_action)
                )
            })
            .unwrap_or_default();

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

#[cfg(test)]
pub mod tests {
    use dashmap::DashSet;
    use reth_provider::test_utils::{ NoopProvider};

    use super::{UserAccountProcessor, UserAccounts};
    use crate::{common::lru_db::RevmLRU, order::state::db_state_utils::test_fetching::MockFetch};

    fn setup_test_account_processor(block: u64) -> UserAccountProcessor<NoopProvider, MockFetch> {
        UserAccountProcessor {
            db:                    Arc::new(RevmLRU::new(100, NoopProvider, 420)),
            user_accounts:         UserAccounts::new(block),
            fetch_utils:           MockFetch::default(),
            known_canceled_orders: DashSet::default()
        }
    }

    #[test]
    fn test_nonce_rejection() {
        let block = 420;
        let mut processor = setup_test_account_processor(block);
    }
}
