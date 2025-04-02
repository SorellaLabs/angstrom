//! keeps track of account state for orders

use alloy::primitives::{Address, B256, U256};
use angstrom_types::{
    orders::OrderId,
    primitive::{UserAccountVerificationError, UserOrderPoolInfo},
    sol_bindings::{ext::RawPoolOrder, grouped_orders::OrderWithStorageData}
};
use user::UserAccounts;

use super::db_state_utils::StateFetchUtils;

pub mod user;

/// processes a user account and tells us based on there current live orders
/// wether or not this order is valid.
pub struct UserAccountProcessor<S> {
    /// keeps track of all user accounts
    user_accounts:          UserAccounts,
    /// utils for fetching the required data to verify
    /// a order.
    pub(crate) fetch_utils: S
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
    ) -> Result<OrderWithStorageData<O>, UserAccountVerificationError> {
        let user = order.from();
        let order_hash = order.order_hash();

        // very nonce hasn't been used historically
        //
        let respend = order.respend_avoidance_strategy();
        match respend {
            angstrom_types::sol_bindings::RespendAvoidanceMethod::Nonce(nonce) => {
                if !self
                    .fetch_utils
                    .is_valid_nonce(user, nonce)
                    .map_err(|e| UserAccountVerificationError::CouldNotFetch(e.to_string()))?
                {
                    return Err(UserAccountVerificationError::DuplicateNonce(order_hash));
                }
            }
            angstrom_types::sol_bindings::RespendAvoidanceMethod::Block(order_block) => {
                // order should be for block + 1
                if block + 1 != order_block {
                    return Err(UserAccountVerificationError::BadBlock(block + 1, order_block));
                }
            }
        }

        // verify that there is not any hooks set.
        if order.has_hook() {
            return Err(UserAccountVerificationError::NonEmptyHook);
        }

        // very we don't have a respend conflict
        let conflicting_orders = self.user_accounts.respend_conflicts(user, respend);
        if conflicting_orders
            .iter()
            .any(|o| o.order_hash <= order_hash)
        {
            // TODO: update this error message because we can't replace order
            // unless the hash is lower. This is simply due to the fact that
            // we need uniformity across all nodes validation in order to properly
            // do slashing as everything needs to be replicable across the board.
            return Err(UserAccountVerificationError::DuplicateNonce(order_hash));
        }
        tracing::trace!(?conflicting_orders);

        // if new order has lower hash cancel all orders with the same nonce
        //
        // TODO: given we are cancelling here. We should notify the user that this
        // order will never be included
        conflicting_orders.iter().for_each(|order| {
            self.user_accounts.cancel_order(&user, &order.order_hash);
        });

        // get the live state sorted up to the nonce, level, doesn't check orders above
        // that
        let live_state = self
            .user_accounts
            .get_live_state_for_order(
                user,
                pool_info.token,
                order.validation_priority(),
                &self.fetch_utils
            )
            .map_err(|e| UserAccountVerificationError::CouldNotFetch(e.to_string()))?;

        // ensure that the current live state is enough to satisfy the order
        match live_state
            .can_support_order(&order, &pool_info)
            .map(|pending_user_action| {
                self.user_accounts
                    .insert_pending_user_action(order.from(), pending_user_action)
            }) {
            Ok(mut invalid_orders) => {
                invalid_orders.extend(conflicting_orders.into_iter().map(|o| o.order_hash));

                Ok(order.into_order_storage_with_data(block, None, true, pool_info, invalid_orders))
            }
            Err(e) => {
                let invalid_orders = conflicting_orders
                    .into_iter()
                    .map(|o| o.order_hash)
                    .collect();
                Ok(order.into_order_storage_with_data(
                    block,
                    Some(e),
                    true,
                    pool_info,
                    invalid_orders
                ))
            }
        }
    }
}

impl<T: RawPoolOrder> StorageWithData for T {}

pub trait StorageWithData: RawPoolOrder {
    fn into_order_storage_with_data(
        self,
        block: u64,
        is_cur_valid: Option<UserAccountVerificationError>,
        is_valid: bool,
        pool_info: UserOrderPoolInfo,
        invalidates: Vec<B256>
    ) -> OrderWithStorageData<Self> {
        OrderWithStorageData {
            priority_data: angstrom_types::orders::OrderPriorityData {
                price:     self.limit_price(),
                // this is always amount as order are collected as
                // bid and ask, thus when compairing, these will all be
                // on the same side
                volume:    self.amount(),
                gas:       U256::ZERO,
                gas_units: 0
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

#[cfg(test)]
pub mod tests {
    use std::collections::HashSet;

    use alloy::primitives::{Address, U256};
    use angstrom_types::{
        primitive::{AngstromSigner, PoolId},
        sol_bindings::{RawPoolOrder, grouped_orders::GroupedVanillaOrder}
    };
    use testing_tools::type_generator::orders::UserOrderBuilder;
    use tracing::info;
    use tracing_subscriber::{EnvFilter, fmt};

    use super::{UserAccountProcessor, UserAccountVerificationError, UserAccounts};
    use crate::order::state::{
        db_state_utils::test_fetching::MockFetch,
        pools::{PoolsTracker, pool_tracker_mock::MockPoolTracker}
    };
    /// Initialize the tracing subscriber for tests
    fn init_tracing() {
        let _ = fmt()
            .with_env_filter(
                EnvFilter::from_default_env()
                    .add_directive("validation=trace".parse().unwrap())
                    .add_directive("info".parse().unwrap())
            )
            .with_test_writer()
            .try_init();
    }

    fn setup_test_account_processor() -> UserAccountProcessor<MockFetch> {
        init_tracing();
        UserAccountProcessor {
            user_accounts: UserAccounts::new(),
            fetch_utils:   MockFetch::default()
        }
    }

    #[test]
    fn test_baseline_order_verification_for_single_order() {
        let processor = setup_test_account_processor();

        let sk = AngstromSigner::random();
        let user = sk.address();

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
            .signing_key(Some(sk.clone()))
            .build();

        // wrap order with details
        let pool_info = mock_pool
            .fetch_pool_info_for_order(&order)
            .expect("pool tracker should have valid state");

        println!("setting balances and approvals");
        processor
            .fetch_utils
            .set_balance_for_user(user, token0, U256::from(order.amount()));
        processor
            .fetch_utils
            .set_approval_for_user(user, token0, U256::from(order.amount()));

        println!("verifying orders");
        processor
            .verify_order(order, pool_info, 420)
            .expect("order should be valid");
    }

    #[test]
    fn test_failure_on_duplicate_pending_nonce() {
        let processor = setup_test_account_processor();

        let sk = AngstromSigner::random();
        let user = sk.address();

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
            .signing_key(Some(sk.clone()))
            .recipient(user)
            .build();

        // wrap order with details
        let pool_info = mock_pool
            .fetch_pool_info_for_order(&order)
            .expect("pool tracker should have valid state");

        processor.fetch_utils.set_balance_for_user(
            user,
            token0,
            U256::from(order.amount()) * U256::from(2)
        );
        processor.fetch_utils.set_approval_for_user(
            user,
            token0,
            U256::from(order.amount()) * U256::from(2)
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
    fn proper_nonce_invalidation_with_lower_nonce_order() {
        let processor = setup_test_account_processor();

        let sk = AngstromSigner::random();
        let user = sk.address();
        info!(?user, "Created random user address");

        let token0 = Address::random();
        let token1 = Address::random();
        info!(?token0, ?token1, "Created random token addresses");

        let mock_pool = MockPoolTracker::default();
        let pool = PoolId::default();

        mock_pool.add_pool(token0, token1, pool);

        let order0: GroupedVanillaOrder = UserOrderBuilder::new()
            .standing()
            .asset_in(token0)
            .asset_out(token1)
            .amount(500)
            .nonce(420)
            .recipient(user)
            .signing_key(Some(sk.clone()))
            .build();
        info!("Created order0 with nonce 420");

        let order1: GroupedVanillaOrder = UserOrderBuilder::new()
            .standing()
            .asset_in(token0)
            .asset_out(token1)
            .amount(500)
            .nonce(90)
            .signing_key(Some(sk.clone()))
            .recipient(user)
            .build();
        info!("Created order1 with nonce 90");
        // wrap order with details
        let pool_info0 = mock_pool
            .fetch_pool_info_for_order(&order0)
            .expect("pool tracker should have valid state");
        let pool_info1 = mock_pool
            .fetch_pool_info_for_order(&order1)
            .expect("pool tracker should have valid state");

        // make it so that no balance
        processor.fetch_utils.set_balance_for_user(
            user,
            token0,
            U256::from(order0.amount()) + U256::from(order1.amount()) - U256::from(10)
        );
        processor.fetch_utils.set_approval_for_user(
            user,
            token0,
            U256::from(order0.amount()) + U256::from(order1.amount()) - U256::from(10)
        );

        let order0_hash = order0.hash();
        let order1_hash = order1.hash();
        info!(?order0_hash, "Generated hash for order0");
        info!(?order1_hash, "Generated hash for order1");

        // first time verifying should pass
        let verify_result0 = processor
            .verify_order(order0, pool_info0, 420)
            .expect("order should be valid");
        info!(?verify_result0, "Verified order0");

        // verify second order and check that order0 hash is in the invalid_orders
        let res = processor
            .verify_order(order1, pool_info1, 420)
            .expect("should be valid");
        info!(?res, "Verified order1");

        info!(
            expected_invalidates = ?vec![order0_hash],
            actual_invalidates = ?res.invalidates,
            "Comparing invalidates vectors"
        );

        assert_eq!(
            res.invalidates,
            vec![order0_hash],
            "order1 should invalidate order0 due to lower nonce"
        );
    }

    #[test]
    fn test_flash_order_block_validation() {
        let processor = setup_test_account_processor();
        let sk = AngstromSigner::random();
        let user = sk.address();
        let token0 = Address::random();
        let token1 = Address::random();
        let mock_pool = MockPoolTracker::default();
        let pool = PoolId::default();
        mock_pool.add_pool(token0, token1, pool);

        // Create flash order for block 421 (current block + 1)
        let order: GroupedVanillaOrder = UserOrderBuilder::new()
            .kill_or_fill()
            .asset_in(token0)
            .asset_out(token1)
            .block(421)
            .recipient(user)
            .signing_key(Some(sk.clone()))
            .build();

        let pool_info = mock_pool
            .fetch_pool_info_for_order(&order)
            .expect("pool tracker should have valid state");

        processor
            .fetch_utils
            .set_balance_for_user(user, token0, U256::from(order.amount()));
        processor
            .fetch_utils
            .set_approval_for_user(user, token0, U256::from(order.amount()));

        // Should succeed for current block 420 (order block is 421)
        processor
            .verify_order(order.clone(), pool_info.clone(), 420)
            .expect("order should be valid for next block");

        // Should fail for wrong current block
        let Err(UserAccountVerificationError::BadBlock(..)) =
            processor.verify_order(order.clone(), pool_info.clone(), 419)
        else {
            panic!("should fail for wrong block");
        };
    }

    #[test]
    fn test_insufficient_balance_invalidation() {
        let processor = setup_test_account_processor();
        let sk = AngstromSigner::random();
        let user = sk.address();
        let token0 = Address::random();
        let token1 = Address::random();
        let mock_pool = MockPoolTracker::default();
        let pool = PoolId::default();
        mock_pool.add_pool(token0, token1, pool);

        let order: GroupedVanillaOrder = UserOrderBuilder::new()
            .standing()
            .asset_in(token0)
            .asset_out(token1)
            .signing_key(Some(sk.clone()))
            .nonce(420)
            .amount(1000)
            .recipient(user)
            .build();

        let h = order.hash();
        info!(?h);
        let pool_info = mock_pool
            .fetch_pool_info_for_order(&order)
            .expect("pool tracker should have valid state");

        // Set balance lower than required
        processor
            .fetch_utils
            .set_balance_for_user(user, token0, U256::from(500));
        processor
            .fetch_utils
            .set_approval_for_user(user, token0, U256::from(1000));

        let result = processor
            .verify_order(order, pool_info, 420)
            .expect("verification should complete");

        assert!(
            !result.is_currently_valid(),
            "Order should be marked as invalid due to insufficient balance {:?}",
            result
        );
    }

    #[test]
    fn test_insufficient_approval_invalidation() {
        let processor = setup_test_account_processor();
        let sk = AngstromSigner::random();
        let user = sk.address();
        let token0 = Address::random();
        let token1 = Address::random();
        let mock_pool = MockPoolTracker::default();
        let pool = PoolId::default();
        mock_pool.add_pool(token0, token1, pool);

        let order: GroupedVanillaOrder = UserOrderBuilder::new()
            .standing()
            .asset_in(token0)
            .asset_out(token1)
            .signing_key(Some(sk.clone()))
            .nonce(420)
            .amount(1000)
            .recipient(user)
            .build();

        let pool_info = mock_pool
            .fetch_pool_info_for_order(&order)
            .expect("pool tracker should have valid state");

        // Set approval lower than required
        processor
            .fetch_utils
            .set_balance_for_user(user, token0, U256::from(1000));
        processor
            .fetch_utils
            .set_approval_for_user(user, token0, U256::from(500));

        let result = processor
            .verify_order(order, pool_info, 420)
            .expect("verification should complete");

        assert!(
            !result.is_currently_valid(),
            "Order should be marked as invalid due to insufficient approval"
        );
    }

    #[test]
    fn test_multiple_orders_same_block() {
        let processor = setup_test_account_processor();
        let sk = AngstromSigner::random();
        let user = sk.address();
        let token0 = Address::random();
        let token1 = Address::random();
        let mock_pool = MockPoolTracker::default();
        let pool = PoolId::default();
        mock_pool.add_pool(token0, token1, pool);

        // Create two flash orders for the same block
        let order1: GroupedVanillaOrder = UserOrderBuilder::new()
            .kill_or_fill()
            .asset_in(token0)
            .asset_out(token1)
            .block(421)
            .signing_key(Some(sk.clone()))
            .amount(500)
            .recipient(user)
            .build();

        let order2: GroupedVanillaOrder = UserOrderBuilder::new()
            .kill_or_fill()
            .asset_in(token0)
            .asset_out(token1)
            .block(421)
            .amount(400)
            .signing_key(Some(sk.clone()))
            .recipient(user)
            .build();

        let pool_info = mock_pool
            .fetch_pool_info_for_order(&order1)
            .expect("pool tracker should have valid state");

        // Set enough balance for both orders
        processor
            .fetch_utils
            .set_balance_for_user(user, token0, U256::from(1000));
        processor
            .fetch_utils
            .set_approval_for_user(user, token0, U256::from(1000));

        // Both orders should be valid
        processor
            .verify_order(order1, pool_info.clone(), 420)
            .expect("first order should be valid");
        processor
            .verify_order(order2, pool_info, 420)
            .expect("second order should be valid");
    }

    #[test]
    fn test_prepare_for_new_block() {
        let processor = setup_test_account_processor();
        let sk = AngstromSigner::random();
        let user = sk.address();
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
            .signing_key(Some(sk.clone()))
            .recipient(user)
            .build();

        let pool_info = mock_pool
            .fetch_pool_info_for_order(&order)
            .expect("pool tracker should have valid state");

        processor
            .fetch_utils
            .set_balance_for_user(user, token0, U256::from(order.amount()));
        processor
            .fetch_utils
            .set_approval_for_user(user, token0, U256::from(order.amount()));

        // Add order
        processor
            .verify_order(order.clone(), pool_info.clone(), 420)
            .expect("order should be valid");

        // Prepare for new block
        processor.prepare_for_new_block(vec![user], vec![order.hash()]);

        // Try to add same order again - should succeed because state was cleared
        let result = processor
            .verify_order(order, pool_info, 420)
            .expect("order should be valid after state clear");

        assert!(result.is_currently_valid(), "Order should be valid after state clear");
    }

    #[test]
    fn test_order_invalidation_chain() {
        let processor = setup_test_account_processor();
        let sk = AngstromSigner::random();
        let user = sk.address();
        let token0 = Address::random();
        let token1 = Address::random();
        let mock_pool = MockPoolTracker::default();
        let pool = PoolId::default();
        mock_pool.add_pool(token0, token1, pool);

        // Create three orders with decreasing nonces
        let order1: GroupedVanillaOrder = UserOrderBuilder::new()
            .standing()
            .asset_in(token0)
            .asset_out(token1)
            .nonce(300)
            .amount(500)
            .recipient(user)
            .signing_key(Some(sk.clone()))
            .build();

        let order2: GroupedVanillaOrder = UserOrderBuilder::new()
            .standing()
            .asset_in(token0)
            .asset_out(token1)
            .nonce(200)
            .amount(500)
            .recipient(user)
            .signing_key(Some(sk.clone()))
            .build();

        let order3: GroupedVanillaOrder = UserOrderBuilder::new()
            .standing()
            .asset_in(token0)
            .asset_out(token1)
            .nonce(100)
            .amount(500)
            .recipient(user)
            .signing_key(Some(sk.clone()))
            .build();

        let pool_info = mock_pool
            .fetch_pool_info_for_order(&order1)
            .expect("pool tracker should have valid state");

        processor
            .fetch_utils
            .set_balance_for_user(user, token0, U256::from(500));
        processor
            .fetch_utils
            .set_approval_for_user(user, token0, U256::from(500));

        // Submit orders in sequence
        let _result1 = processor
            .verify_order(order1.clone(), pool_info.clone(), 420)
            .expect("first order should be valid");
        let result2 = processor
            .verify_order(order2.clone(), pool_info.clone(), 420)
            .expect("second order should be valid");

        let result3 = processor
            .verify_order(order3, pool_info, 420)
            .expect("third order should be valid");

        // Verify that each order invalidates all previous orders
        assert!(result2.invalidates.contains(&order1.hash()));
        assert!(result3.invalidates.contains(&order2.hash()));
        assert!(result3.invalidates.contains(&order1.hash()));
    }

    #[test]
    fn test_balance_sharing_between_orders() {
        let processor = setup_test_account_processor();
        let sk = AngstromSigner::random();
        let user = sk.address();
        let token0 = Address::random();
        let token1 = Address::random();
        let mock_pool = MockPoolTracker::default();
        let pool = PoolId::default();
        mock_pool.add_pool(token0, token1, pool);

        // Create two orders that together exceed available balance
        let order1: GroupedVanillaOrder = UserOrderBuilder::new()
            .standing()
            .asset_in(token0)
            .asset_out(token1)
            .nonce(100)
            .amount(600)
            .recipient(user)
            .signing_key(Some(sk.clone()))
            .build();

        let order2: GroupedVanillaOrder = UserOrderBuilder::new()
            .standing()
            .asset_in(token0)
            .asset_out(token1)
            .nonce(101)
            .amount(500)
            .recipient(user)
            .signing_key(Some(sk.clone()))
            .build();

        let pool_info = mock_pool
            .fetch_pool_info_for_order(&order1)
            .expect("pool tracker should have valid state");

        // Set balance that's enough for first order but not both
        processor
            .fetch_utils
            .set_balance_for_user(user, token0, U256::from(800));
        processor
            .fetch_utils
            .set_approval_for_user(user, token0, U256::from(1500));

        let result1 = processor
            .verify_order(order1, pool_info.clone(), 420)
            .expect("first order should be valid");

        let result2 = processor
            .verify_order(order2, pool_info, 420)
            .expect("second order should complete verification");

        assert!(result1.is_currently_valid(), "First order should be valid");

        assert!(
            !result2.is_currently_valid(),
            "Second order should be invalid due to insufficient remaining balance"
        );
    }

    #[test]
    fn test_flash_order_sequence() {
        let processor = setup_test_account_processor();
        let sk = AngstromSigner::random();
        let user = sk.address();
        let token0 = Address::random();
        let token1 = Address::random();
        let mock_pool = MockPoolTracker::default();
        let pool = PoolId::default();
        mock_pool.add_pool(token0, token1, pool);

        // Create sequence of flash orders for consecutive blocks
        let order1: GroupedVanillaOrder = UserOrderBuilder::new()
            .kill_or_fill()
            .asset_in(token0)
            .asset_out(token1)
            .block(421)
            .amount(500)
            .recipient(user)
            .signing_key(Some(sk.clone()))
            .build();

        let order2: GroupedVanillaOrder = UserOrderBuilder::new()
            .kill_or_fill()
            .asset_in(token0)
            .asset_out(token1)
            .block(422)
            .amount(500)
            .recipient(user)
            .signing_key(Some(sk.clone()))
            .build();

        let pool_info = mock_pool
            .fetch_pool_info_for_order(&order1)
            .expect("pool tracker should have valid state");

        processor
            .fetch_utils
            .set_balance_for_user(user, token0, U256::from(1000));
        processor
            .fetch_utils
            .set_approval_for_user(user, token0, U256::from(1000));

        // Verify orders for their respective blocks
        let result1 = processor
            .verify_order(order1, pool_info.clone(), 420)
            .expect("first order should be valid");
        let result2 = processor
            .verify_order(order2, pool_info, 421)
            .expect("second order should be valid");

        assert!(result1.is_currently_valid(), "First flash order should be valid");
        assert!(result2.is_currently_valid(), "Second flash order should be valid");
    }

    #[test]
    fn test_mixed_order_types() {
        let processor = setup_test_account_processor();
        let sk = AngstromSigner::random();
        let user = sk.address();
        let token0 = Address::random();
        let token1 = Address::random();
        let mock_pool = MockPoolTracker::default();
        let pool = PoolId::default();
        mock_pool.add_pool(token0, token1, pool);

        // Create mix of standing and flash orders
        let standing_order: GroupedVanillaOrder = UserOrderBuilder::new()
            .standing()
            .asset_in(token0)
            .asset_out(token1)
            .nonce(100)
            .amount(500)
            .recipient(user)
            .signing_key(Some(sk.clone()))
            .build();

        let flash_order: GroupedVanillaOrder = UserOrderBuilder::new()
            .kill_or_fill()
            .asset_in(token0)
            .asset_out(token1)
            .block(421)
            .amount(500)
            .recipient(user)
            .signing_key(Some(sk.clone()))
            .build();

        let pool_info = mock_pool
            .fetch_pool_info_for_order(&standing_order)
            .expect("pool tracker should have valid state");

        processor
            .fetch_utils
            .set_balance_for_user(user, token0, U256::from(1000));
        processor
            .fetch_utils
            .set_approval_for_user(user, token0, U256::from(1000));

        let standing_result = processor
            .verify_order(standing_order, pool_info.clone(), 420)
            .expect("standing order should be valid");
        let flash_result = processor
            .verify_order(flash_order, pool_info, 420)
            .expect("flash order should be valid");

        assert!(standing_result.is_currently_valid(), "Standing order should be valid");
        assert!(flash_result.is_currently_valid(), "Flash order should be valid");
    }

    #[test]
    fn test_nonce_rejection() {
        let processor = setup_test_account_processor();
        let token0 = Address::random();
        let token1 = Address::random();
        let mock_pool = MockPoolTracker::default();
        let pool = PoolId::default();

        let sk = AngstromSigner::random();
        let user = sk.address();

        mock_pool.add_pool(token0, token1, pool);

        let order: GroupedVanillaOrder = UserOrderBuilder::new()
            .standing()
            .recipient(user)
            .asset_in(token0)
            .asset_out(token1)
            .nonce(420)
            .recipient(user)
            .amount(1000)
            .signing_key(Some(sk.clone()))
            .build();

        // wrap order with details
        let pool_info = mock_pool
            .fetch_pool_info_for_order(&order)
            .expect("pool tracker should have valid state");

        // Set up proper balance and approval
        processor
            .fetch_utils
            .set_balance_for_user(user, token0, U256::from(order.amount()));
        processor
            .fetch_utils
            .set_approval_for_user(user, token0, U256::from(order.amount()));

        // Mark nonce as already used
        processor
            .fetch_utils
            .set_used_nonces(user, HashSet::from([420]));

        // Verify the order fails due to duplicate nonce
        let result = processor.verify_order(order, pool_info, 420);

        // Assert we get the expected error
        assert!(
            matches!(result, Err(UserAccountVerificationError::DuplicateNonce(..))),
            "Expected DuplicateNonce error, got {:?}",
            result
        );
    }
}
