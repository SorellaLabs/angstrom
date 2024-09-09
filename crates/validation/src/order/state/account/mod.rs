//! keeps track of account state for orders
use std::{
    collections::HashSet,
    sync::{atomic::AtomicU64, Arc}
};

use alloy_primitives::{Address, B256};
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
    pub fn new(db: Arc<RevmLRU<DB>>, current_block: u64, fetch_utils: S) -> Self {
        let user_accounts = UserAccounts::new(current_block);
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

    pub fn prepare_for_new_block(&self, users: Vec<Address>, orders: Vec<B256>) {
        orders.iter().for_each(|order| {
            self.known_canceled_orders.remove(order);
        });

        self.user_accounts.new_block(users, orders);
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

        // see if order has been cancelled before
        if self.known_canceled_orders.contains(&order_hash) {
            return Err(UserAccountVerificationError::OrderIsCancelled(order_hash))
        }

        println!("getting live state");
        let live_state = self.user_accounts.get_live_state_for_order(
            user,
            pool_info.token,
            nonce,
            &self.fetch_utils,
            &self.db
        );
        println!("live state got");

        // ensure that the current live state is enough to satisfy the order
        let (is_cur_valid, invalid_orders) = live_state
            .can_support_order(&order, &pool_info)
            .map(|pending_user_action| {
                println!("can support order map");
                (
                    true,
                    self.user_accounts
                        .insert_pending_user_action(order.from(), pending_user_action)
                )
            })
            .unwrap_or_default();
        println!("got if we can support");

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
    use std::{
        collections::HashSet,
        sync::{atomic::AtomicU64, Arc}
    };

    use alloy_primitives::U256;
    use angstrom_types::sol_bindings::grouped_orders::{GroupedVanillaOrder, PoolOrder};
    use dashmap::DashSet;
    use rand::thread_rng;
    use reth_primitives::Address;
    use reth_provider::test_utils::NoopProvider;
    use revm::primitives::bitvec::store::BitStore;
    use testing_tools::type_generator::orders::generate_limit_order;

    use super::{UserAccountProcessor, UserAccountVerificationError, UserAccounts};
    use crate::{
        common::lru_db::RevmLRU,
        order::state::{
            db_state_utils::test_fetching::MockFetch,
            pools::{pool_tracker_mock::MockPoolTracker, PoolsTracker}
        }
    };

    fn setup_test_account_processor(block: u64) -> UserAccountProcessor<NoopProvider, MockFetch> {
        UserAccountProcessor {
            db:                    Arc::new(RevmLRU::new(
                100,
                Arc::new(NoopProvider::default()),
                Arc::new(AtomicU64::new(420))
            )),
            user_accounts:         UserAccounts::new(block),
            fetch_utils:           MockFetch::default(),
            known_canceled_orders: DashSet::default()
        }
    }

    #[test]
    fn test_baseline_order_verification_for_single_order() {
        let block = 420;
        let mut processor = setup_test_account_processor(block);

        let user = Address::random();
        let asset0 = 0;
        let asset1 = 1;

        let token0 = Address::random();
        let token1 = Address::random();
        let pool = 10;

        let mut mock_pool = MockPoolTracker::default();

        mock_pool.add_pool(token0, token1, pool);
        mock_pool.add_asset(asset0, token0);
        mock_pool.add_asset(asset1, token1);

        let mut rng = thread_rng();
        let mut order: GroupedVanillaOrder = generate_limit_order(
            &mut rng,
            false,
            true,
            Some(pool as usize),
            None,
            Some(asset0),
            Some(asset1),
            Some(420),
            Some(user)
        )
        .order;

        // wrap order with details
        let (pool_info, order) = mock_pool
            .fetch_pool_info_for_order(order)
            .expect("pool tracker should have valid state");

        println!("setting balances and approvals");
        processor
            .fetch_utils
            .set_balance_for_user(user, token0, U256::from(order.amount_in()));
        processor
            .fetch_utils
            .set_approval_for_user(user, token0, U256::from(order.amount_in()));

        println!("verifying orders");
        processor
            .verify_order(order, pool_info, 420, true)
            .expect("order should be valid");
    }

    #[test]
    fn test_failure_on_duplicate_pending_nonce() {
        let block = 420;
        let mut processor = setup_test_account_processor(block);

        let user = Address::random();
        let asset0 = 0;
        let asset1 = 1;

        let token0 = Address::random();
        let token1 = Address::random();
        let pool = 10;

        let mut mock_pool = MockPoolTracker::default();

        mock_pool.add_pool(token0, token1, pool);
        mock_pool.add_asset(asset0, token0);
        mock_pool.add_asset(asset1, token1);

        let mut rng = thread_rng();
        let mut order: GroupedVanillaOrder = generate_limit_order(
            &mut rng,
            false,
            true,
            Some(pool as usize),
            None,
            Some(asset0),
            Some(asset1),
            Some(420),
            Some(user)
        )
        .order;

        // wrap order with details
        let (pool_info, order) = mock_pool
            .fetch_pool_info_for_order(order)
            .expect("pool tracker should have valid state");

        processor.fetch_utils.set_balance_for_user(
            user,
            token0,
            U256::from(order.amount_in()) * U256::from(2)
        );
        processor.fetch_utils.set_approval_for_user(
            user,
            token0,
            U256::from(order.amount_in()) * U256::from(2)
        );

        println!("finished first order config");
        // first time verifying should pass
        processor
            .verify_order(order.clone(), pool_info.clone(), 420, true)
            .expect("order should be valid");

        println!("first order has been set valid");
        // second time should fail
        let Err(e) = processor.verify_order(order, pool_info, 420, true) else { panic!("verifying order should of failed")};
        assert!(matches!(e, UserAccountVerificationError::DuplicateNonce(..)));
    }

    #[test]
    fn test_nonce_rejection() {
        let block = 420;
        let mut processor = setup_test_account_processor(block);

        let user = Address::random();
        let asset0 = 0;
        let asset1 = 1;

        let token0 = Address::random();
        let token1 = Address::random();
        let pool = 10;

        let mut mock_pool = MockPoolTracker::default();

        mock_pool.add_pool(token0, token1, pool);
        mock_pool.add_asset(asset0, token0);
        mock_pool.add_asset(asset1, token1);

        let mut rng = thread_rng();
        let mut order: GroupedVanillaOrder = generate_limit_order(
            &mut rng,
            false,
            true,
            Some(pool as usize),
            None,
            Some(asset0),
            Some(asset1),
            Some(420),
            Some(user)
        )
        .order;

        // wrap order with details
        let (pool_info, order) = mock_pool
            .fetch_pool_info_for_order(order)
            .expect("pool tracker should have valid state");

        processor
            .fetch_utils
            .set_used_nonces(user, HashSet::from([420]));

        let Err(e) = processor.verify_order(order, pool_info, 420, true) else { panic!("verifying order should of failed")};

        assert!(matches!(e, UserAccountVerificationError::DuplicateNonce(..)));
    }
}
