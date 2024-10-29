//! keeps track of account state for orders

use alloy::primitives::{Address, B256, U256};
use angstrom_types::{
    orders::OrderId,
    sol_bindings::{ext::RawPoolOrder, grouped_orders::OrderWithStorageData}
};
use thiserror::Error;
use user::UserAccounts;

use super::{db_state_utils::StateFetchUtils, pools::UserOrderPoolInfo};

pub mod user;

/// processes a user account and tells us based on there current live orders
/// wether or not this order is valid.
pub struct UserAccountProcessor<S> {
    /// keeps track of all user accounts
    user_accounts: UserAccounts,
    /// utils for fetching the required data to verify
    /// a order.
    fetch_utils:   S
}

impl<S: StateFetchUtils> UserAccountProcessor<S> {
    pub fn new(fetch_utils: S) -> Self {
        let user_accounts = UserAccounts::new();
        Self { fetch_utils, user_accounts }
    }

    pub fn prepare_for_new_block(&self, users: Vec<Address>, orders: Vec<B256>) {
        self.user_accounts.new_block(users, orders);
    }

    pub fn verify_order<O: RawPoolOrder>(
        &self,
        order: O,
        pool_info: UserOrderPoolInfo,
        block: u64
    ) -> Result<OrderWithStorageData<O>, UserAccountVerificationError<O>> {
        let user = order.from();
        let order_hash = order.order_hash();

        // very nonce hasn't been used historically
        //
        let respend = order.respend_avoidance_strategy();
        match respend {
            angstrom_types::sol_bindings::RespendAvoidanceMethod::Nonce(nonce) => {
                if !self.fetch_utils.is_valid_nonce(user, nonce) {
                    return Err(UserAccountVerificationError::DuplicateNonce(order_hash))
                }
            }
            angstrom_types::sol_bindings::RespendAvoidanceMethod::Block(order_block) => {
                if block != order_block {
                    return Err(UserAccountVerificationError::BadBlock)
                }
            }
        }

        // very we don't have a respend conflict
        let conflicting_orders = self.user_accounts.respend_conflicts(user, respend);
        if conflicting_orders
            .iter()
            .any(|o| o.order_hash <= order_hash)
        {
            return Err(UserAccountVerificationError::DuplicateNonce(order_hash))
        }
        // if new order has lower hash cancel all orders with the same nonce
        conflicting_orders.iter().for_each(|order| {
            self.user_accounts.cancel_order(&user, &order.order_hash);
        });

        let live_state = self.user_accounts.get_live_state_for_order(
            user,
            pool_info.token,
            respend,
            &self.fetch_utils
        );

        // ensure that the current live state is enough to satisfy the order
        let (is_cur_valid, mut invalid_orders) = live_state
            .can_support_order(&order, &pool_info)
            .map(|pending_user_action| {
                (
                    true,
                    self.user_accounts
                        .insert_pending_user_action(order.from(), pending_user_action)
                )
            })
            .unwrap_or_default();

        // invalidate orders with clashing nonces
        invalid_orders.extend(conflicting_orders.into_iter().map(|o| o.order_hash));

        Ok(order.into_order_storage_with_data(block, is_cur_valid, true, pool_info, invalid_orders))
    }
}

impl<T: RawPoolOrder> StorageWithData for T {}

pub trait StorageWithData: RawPoolOrder {
    fn into_order_storage_with_data(
        self,
        block: u64,
        is_cur_valid: bool,
        is_valid: bool,
        pool_info: UserOrderPoolInfo,
        invalidates: Vec<B256>
    ) -> OrderWithStorageData<Self> {
        OrderWithStorageData {
            priority_data: angstrom_types::orders::OrderPriorityData {
                price:  self.limit_price(),
                volume: self.amount_in(),
                gas:    0
            },
            pool_id: pool_info.pool_id,
            is_currently_valid: is_cur_valid,
            is_bid: pool_info.is_bid,
            is_valid,
            valid_block: block,
            order_id: OrderId::from_all_orders(&self, pool_info.pool_id),
            invalidates,
            order: self,
            tob_reward: U256::ZERO
        }
    }
}

#[derive(Debug, Error)]
pub enum UserAccountVerificationError<O: RawPoolOrder> {
    #[error("tried to verify for block {} where current is {}", requested, current)]
    BlockMissMatch { requested: u64, current: u64, order: O, pool_info: UserOrderPoolInfo },
    #[error("order hash has been cancelled {0:?}")]
    OrderIsCancelled(B256),
    #[error("Nonce exists for a current order hash: {0:?}")]
    DuplicateNonce(B256),
    #[error("block for flash order is not current block")]
    BadBlock
}

#[cfg(test)]
pub mod tests {
    use std::collections::HashSet;

    use alloy::primitives::{Address, U256};
    use angstrom_types::{
        primitive::PoolId,
        sol_bindings::{grouped_orders::GroupedVanillaOrder, RawPoolOrder}
    };
    use testing_tools::type_generator::orders::UserOrderBuilder;

    use super::{UserAccountProcessor, UserAccountVerificationError, UserAccounts};
    use crate::order::state::{
        db_state_utils::test_fetching::MockFetch,
        pools::{pool_tracker_mock::MockPoolTracker, PoolsTracker}
    };

    fn setup_test_account_processor() -> UserAccountProcessor<MockFetch> {
        UserAccountProcessor {
            user_accounts: UserAccounts::new(),
            fetch_utils:   MockFetch::default()
        }
    }

    #[test]
    fn test_baseline_order_verification_for_single_order() {
        let processor = setup_test_account_processor();

        let user = Address::random();

        let token0 = Address::random();
        let token1 = Address::random();

        let mock_pool = MockPoolTracker::default();

        let pool = PoolId::default();

        mock_pool.add_pool(token0, token1, pool);

        let order: GroupedVanillaOrder = UserOrderBuilder::new()
            .standing()
            .asset_in(token0)
            .asset_out(token1)
            .nonce(420)
            .recipient(user)
            .build();

        // wrap order with details
        let pool_info = mock_pool
            .fetch_pool_info_for_order(&order)
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
            .verify_order(order, pool_info, 420)
            .expect("order should be valid");
    }

    #[test]
    fn test_failure_on_duplicate_pending_nonce() {
        let processor = setup_test_account_processor();

        let user = Address::random();

        let token0 = Address::random();
        let token1 = Address::random();

        let mock_pool = MockPoolTracker::default();
        let pool = PoolId::default();

        mock_pool.add_pool(token0, token1, pool);

        let order: GroupedVanillaOrder = UserOrderBuilder::new()
            .standing()
            .asset_in(token0)
            .asset_out(token1)
            .nonce(420)
            .recipient(user)
            .build();

        // wrap order with details
        let pool_info = mock_pool
            .fetch_pool_info_for_order(&order)
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
            .verify_order(order.clone(), pool_info.clone(), 420)
            .expect("order should be valid");

        println!("first order has been set valid");
        // second time should fail
        let Err(e) = processor.verify_order(order, pool_info, 420) else {
            panic!("verifying order should of failed")
        };
        assert!(matches!(e, UserAccountVerificationError::DuplicateNonce(..)));
    }

    #[test]
    fn test_order_replacement_on_lower_nonce() {
        let processor = setup_test_account_processor();

        let user = Address::random();

        let token0 = Address::random();
        let token1 = Address::random();

        let mock_pool = MockPoolTracker::default();
        let pool = PoolId::default();

        mock_pool.add_pool(token0, token1, pool);

        let order0: GroupedVanillaOrder = UserOrderBuilder::new()
            .standing()
            .asset_in(token0)
            .asset_out(token1)
            .nonce(420)
            .recipient(user)
            .build();
        let order1: GroupedVanillaOrder = UserOrderBuilder::new()
            .standing()
            .asset_in(token0)
            .asset_out(token1)
            .nonce(90)
            .recipient(user)
            .build();
        // wrap order with details
        let pool_info0 = mock_pool
            .fetch_pool_info_for_order(&order0)
            .expect("pool tracker should have valid state");
        let pool_info1 = mock_pool
            .fetch_pool_info_for_order(&order1)
            .expect("pool tracker should have valid state");

        processor.fetch_utils.set_balance_for_user(
            user,
            token0,
            U256::from(order0.amount_in()) + U256::from(order1.amount_in()) - U256::from(10)
        );
        processor.fetch_utils.set_approval_for_user(
            user,
            token0,
            U256::from(order0.amount_in()) + U256::from(order1.amount_in()) - U256::from(10)
        );

        let order0_hash = order0.hash();
        // first time verifying should pass
        processor
            .verify_order(order0, pool_info0, 420)
            .expect("order should be valid");

        // very second order and that order0 hash is in the invalid_orders
        // second time should fail
        let res = processor
            .verify_order(order1, pool_info1, 420)
            .expect("should be valid");
        assert_eq!(res.invalidates, vec![order0_hash]);
    }

    #[test]
    fn test_nonce_rejection() {
        let processor = setup_test_account_processor();

        let user = Address::random();

        let token0 = Address::random();
        let token1 = Address::random();

        let mock_pool = MockPoolTracker::default();
        let pool = PoolId::default();

        mock_pool.add_pool(token0, token1, pool);

        let order: GroupedVanillaOrder = UserOrderBuilder::new()
            .standing()
            .asset_in(token0)
            .asset_out(token1)
            .nonce(420)
            .recipient(user)
            .build();

        // wrap order with details
        let pool_info = mock_pool
            .fetch_pool_info_for_order(&order)
            .expect("pool tracker should have valid state");

        processor
            .fetch_utils
            .set_used_nonces(user, HashSet::from([420]));

        let Err(e) = processor.verify_order(order, pool_info, 420) else {
            panic!("verifying order should of failed")
        };

        assert!(matches!(e, UserAccountVerificationError::DuplicateNonce(..)));
    }
}
