use std::{collections::HashMap, sync::Arc};

use alloy::primitives::{Address, B256, U256};
use angstrom_types::sol_bindings::{ext::RawPoolOrder, RespendAvoidanceMethod};
use dashmap::DashMap;

use crate::order::state::{db_state_utils::StateFetchUtils, pools::UserOrderPoolInfo};

pub type UserAddress = Address;
pub type TokenAddress = Address;
pub type Amount = U256;

#[derive(Debug, Default)]
pub struct BaselineState {
    token_approval:   HashMap<TokenAddress, Amount>,
    token_balance:    HashMap<TokenAddress, Amount>,
    angstrom_balance: HashMap<TokenAddress, Amount>
}

#[derive(Debug)]
pub struct LiveState {
    pub token:            TokenAddress,
    pub approval:         Amount,
    pub balance:          Amount,
    pub angstrom_balance: Amount
}

impl LiveState {
    pub fn can_support_order<O: RawPoolOrder>(
        &self,
        order: &O,
        pool_info: &UserOrderPoolInfo
    ) -> Option<PendingUserAction> {
        assert_eq!(order.token_in(), self.token, "incorrect lives state for order");
        let amount_in = U256::from(order.amount_in());

        let (angstrom_delta, token_delta) = if order.use_internal() {
            if self.angstrom_balance < amount_in {
                return None;
            }
            (amount_in, U256::ZERO)
        } else {
            if self.approval < amount_in || self.balance < amount_in {
                return None;
            }
            (U256::ZERO, amount_in)
        };

        Some(PendingUserAction {
            order_hash: order.order_hash(),
            respend: order.respend_avoidance_strategy(),
            token_address: pool_info.token,
            token_delta,
            angstrom_delta,
            token_approval: amount_in,
            pool_info: pool_info.clone()
        })
    }
}

/// deltas to be applied to the base user action
#[derive(Clone, Debug)]
pub struct PendingUserAction {
    /// hash of order
    pub order_hash:     B256,
    pub respend:        RespendAvoidanceMethod,
    // for each order, there will be two different deltas
    pub token_address:  TokenAddress,
    // although we have deltas for two tokens, we only
    // apply for 1 given the execution of angstrom,
    // all tokens are required before execution.
    pub token_delta:    Amount,
    pub token_approval: Amount,
    // balance spent from angstrom
    pub angstrom_delta: Amount,

    pub pool_info: UserOrderPoolInfo
}

pub struct UserAccounts {
    /// all of a user addresses pending orders.
    pending_actions: Arc<DashMap<UserAddress, Vec<PendingUserAction>>>,

    /// the last updated state of a given user.
    last_known_state: Arc<DashMap<UserAddress, BaselineState>>
}

impl Default for UserAccounts {
    fn default() -> Self {
        Self::new()
    }
}

impl UserAccounts {
    pub fn new() -> Self {
        Self {
            pending_actions:  Arc::new(DashMap::default()),
            last_known_state: Arc::new(DashMap::default())
        }
    }

    pub fn new_block(&self, users: Vec<Address>, orders: Vec<B256>) {
        // remove all user specific orders
        users.iter().for_each(|user| {
            self.pending_actions.remove(user);
            self.last_known_state.remove(user);
        });

        // remove all singular orders
        self.pending_actions.retain(|_, pending_orders| {
            pending_orders.retain(|p| !orders.contains(&p.order_hash));
            !pending_orders.is_empty()
        });
    }

    /// returns true if the order cancel has been processed successfully
    pub fn cancel_order(&self, user: &UserAddress, order_hash: &B256) -> bool {
        let Some(mut inner_orders) = self.pending_actions.get_mut(user) else { return false };
        let mut res = false;

        inner_orders.retain(|o| {
            let matches = &o.order_hash != order_hash;
            res |= !matches;
            matches
        });

        res
    }

    pub fn respend_conflicts(
        &self,
        user: UserAddress,
        avoidance: RespendAvoidanceMethod
    ) -> Vec<PendingUserAction> {
        match avoidance {
            nonce @ RespendAvoidanceMethod::Nonce(_) => self
                .pending_actions
                .get(&user)
                .map(|v| {
                    v.value()
                        .iter()
                        .filter(|pending_order| pending_order.respend == nonce)
                        .cloned()
                        .collect()
                })
                .unwrap_or_default(),
            RespendAvoidanceMethod::Block(_) => Vec::new()
        }
    }

    pub fn get_live_state_for_order<S: StateFetchUtils>(
        &self,
        user: UserAddress,
        token: TokenAddress,
        respend: RespendAvoidanceMethod,
        utils: &S
    ) -> LiveState {
        self.try_fetch_live_pending_state(user, token, respend)
            .unwrap_or_else(|| {
                self.load_state_for(user, token, utils);
                self.try_fetch_live_pending_state(user, token, respend)
                    .expect(
                        "after loading state for a address, the state wasn't found. this should \
                         be impossible"
                    )
            })
    }

    fn load_state_for<S: StateFetchUtils>(
        &self,
        user: UserAddress,
        token: TokenAddress,
        utils: &S
    ) {
        let approvals = utils
            .fetch_approval_balance_for_token(user, token)
            .unwrap_or_default();
        let balances = utils.fetch_balance_for_token(user, token);

        let mut entry = self.last_known_state.entry(user).or_default();
        // override as fresh query
        entry.token_balance.insert(token, balances);
        entry.token_approval.insert(token, approvals);
        entry.angstrom_balance.insert(token, balances);
    }

    /// inserts the user action and returns all pending user action hashes that
    /// this, invalidates. i.e higher nonce but no balance / approval
    /// available.
    pub fn insert_pending_user_action(
        &self,
        user: UserAddress,
        action: PendingUserAction
    ) -> Vec<B256> {
        let token = action.token_address;
        let mut entry = self.pending_actions.entry(user).or_default();
        let value = entry.value_mut();

        value.push(action);
        value.sort_unstable_by_key(|k| k.respend.get_ord_for_pending_orders());
        drop(entry);

        // iterate through all vales collected the orders that
        self.fetch_all_invalidated_orders(user, token)
    }

    fn fetch_all_invalidated_orders(&self, user: UserAddress, token: TokenAddress) -> Vec<B256> {
        let Some(baseline) = self.last_known_state.get(&user) else { return vec![] };

        let mut baseline_approval = *baseline.token_approval.get(&token).unwrap();
        let mut baseline_balance = *baseline.token_balance.get(&token).unwrap();
        let mut baseline_angstrom_balance = *baseline.angstrom_balance.get(&token).unwrap();
        let mut has_overflowed = false;

        let mut bad = vec![];
        for pending_state in self
            .pending_actions
            .get(&user)
            .unwrap()
            .iter()
            .filter(|state| state.token_address == token)
        {
            let (baseline, overflowed) =
                baseline_approval.overflowing_sub(pending_state.token_approval);
            has_overflowed |= overflowed;
            baseline_approval = baseline;

            let (baseline, overflowed) =
                baseline_balance.overflowing_sub(pending_state.token_delta);
            has_overflowed |= overflowed;
            baseline_balance = baseline;

            let (baseline, overflowed) =
                baseline_angstrom_balance.overflowing_sub(pending_state.angstrom_delta);
            has_overflowed |= overflowed;
            baseline_angstrom_balance = baseline;

            // mark for removal
            if has_overflowed {
                bad.push(pending_state.order_hash);
            }
        }
        bad
    }

    /// for the given user and token_in, and nonce, will return none
    /// if there is no baseline information for the given user
    /// account.
    fn try_fetch_live_pending_state(
        &self,
        user: UserAddress,
        token: TokenAddress,
        respend: RespendAvoidanceMethod
    ) -> Option<LiveState> {
        let baseline = self.last_known_state.get(&user)?;
        let baseline_approval = *baseline.token_approval.get(&token)?;
        let baseline_balance = *baseline.token_balance.get(&token)?;
        let baseline_angstrom_balance = *baseline.angstrom_balance.get(&token)?;

        // the values returned here are the negative delta compaired to baseline.
        let (pending_approvals_spend, pending_balance_spend, pending_angstrom_balance_spend) = self
            .pending_actions
            .get(&user)
            .map(|val| {
                val.iter()
                    .filter(|state| state.token_address == token)
                    .take_while(|state| {
                        state.respend.get_ord_for_pending_orders()
                            <= respend.get_ord_for_pending_orders()
                    })
                    .fold(
                        (Amount::default(), Amount::default(), Amount::default()),
                        |(mut approvals_spend, mut balance_spend, mut angstrom_spend), x| {
                            approvals_spend += x.token_approval;
                            balance_spend += x.token_delta;
                            angstrom_spend += x.angstrom_delta;
                            (approvals_spend, balance_spend, angstrom_spend)
                        }
                    )
            })
            .unwrap_or_default();

        let live_approval = baseline_approval.saturating_sub(pending_approvals_spend);
        let live_balance = baseline_balance.saturating_sub(pending_balance_spend);
        let live_angstrom_balance =
            baseline_angstrom_balance.saturating_sub(pending_angstrom_balance_spend);

        Some(LiveState {
            token,
            balance: live_balance,
            approval: live_approval,
            angstrom_balance: live_angstrom_balance
        })
    }
}

#[cfg(test)]
mod tests {
    use alloy::primitives::address;

    use super::*;

    fn setup_test_accounts() -> UserAccounts {
        UserAccounts::new()
    }

    fn create_test_pending_action(
        token: TokenAddress,
        token_delta: U256,
        angstrom_delta: U256,
        token_approval: U256,
        nonce: u64
    ) -> PendingUserAction {
        PendingUserAction {
            order_hash: B256::random(),
            respend: RespendAvoidanceMethod::Nonce(nonce),
            token_address: token,
            token_delta,
            token_approval,
            angstrom_delta,
            pool_info: UserOrderPoolInfo { token, ..Default::default() }
        }
    }

    #[test]
    fn test_new_block_handling() {
        let accounts = setup_test_accounts();
        let user1 = address!("1234567890123456789012345678901234567890");
        let user2 = address!("2234567890123456789012345678901234567890");
        let token = address!("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef");

        // Add some pending actions
        let action1 =
            create_test_pending_action(token, U256::from(100), U256::from(0), U256::from(100), 1);
        let action2 =
            create_test_pending_action(token, U256::from(200), U256::from(0), U256::from(200), 2);

        accounts.insert_pending_user_action(user1, action1.clone());
        accounts.insert_pending_user_action(user2, action2.clone());

        // Verify actions are present
        assert!(accounts.pending_actions.contains_key(&user1));
        assert!(accounts.pending_actions.contains_key(&user2));

        // Call new_block to clear specific users and orders
        accounts.new_block(vec![user1], vec![action2.order_hash]);

        // Verify user1's actions are cleared
        assert!(!accounts.pending_actions.contains_key(&user1));
        // Verify user2's actions with matching order_hash are cleared
        assert!(accounts
            .pending_actions
            .get(&user2)
            .map_or(true, |actions| actions.is_empty()));
    }

    #[test]
    fn test_cancel_order() {
        let accounts = setup_test_accounts();
        let user = address!("1234567890123456789012345678901234567890");
        let token = address!("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef");

        let action =
            create_test_pending_action(token, U256::from(100), U256::from(0), U256::from(100), 1);
        let order_hash = action.order_hash;

        accounts.insert_pending_user_action(user, action);

        // Test canceling non-existent order
        assert!(!accounts.cancel_order(&user, &B256::random()));

        // Test canceling existing order
        assert!(accounts.cancel_order(&user, &order_hash));

        // Verify order was removed
        assert!(accounts
            .pending_actions
            .get(&user)
            .map_or(true, |actions| actions.is_empty()));
    }

    #[test]
    fn test_respend_conflicts() {
        let accounts = setup_test_accounts();
        let user = address!("1234567890123456789012345678901234567890");
        let token = address!("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef");

        // Test with empty state first
        let conflicts = accounts.respend_conflicts(user, RespendAvoidanceMethod::Nonce(1));
        assert!(conflicts.is_empty(), "Should return empty vec when no actions exist");

        // Create actions with same nonce
        let action1 =
            create_test_pending_action(token, U256::from(100), U256::from(0), U256::from(100), 1);
        let action2 =
            create_test_pending_action(token, U256::from(200), U256::from(0), U256::from(200), 1);
        let action3 =
            create_test_pending_action(token, U256::from(300), U256::from(0), U256::from(300), 2);

        // Insert actions one by one and verify
        accounts.insert_pending_user_action(user, action1.clone());
        let conflicts = accounts.respend_conflicts(user, RespendAvoidanceMethod::Nonce(1));
        assert_eq!(conflicts.len(), 1, "Should find one conflict for nonce 1");

        accounts.insert_pending_user_action(user, action2.clone());
        let conflicts = accounts.respend_conflicts(user, RespendAvoidanceMethod::Nonce(1));
        assert_eq!(conflicts.len(), 2, "Should find two conflicts for nonce 1");

        accounts.insert_pending_user_action(user, action3.clone());
        let conflicts = accounts.respend_conflicts(user, RespendAvoidanceMethod::Nonce(2));
        assert_eq!(conflicts.len(), 1, "Should find one conflict for nonce 2");

        // Test block-based respend avoidance
        let conflicts = accounts.respend_conflicts(user, RespendAvoidanceMethod::Block(1));
        assert!(conflicts.is_empty(), "Block-based respend should return empty vec");
    }

    #[test]
    fn test_live_state_calculation() {
        let accounts = setup_test_accounts();
        let user = address!("1234567890123456789012345678901234567890");
        let token = address!("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef");

        // Set up initial state
        let mut baseline = BaselineState::default();
        baseline.token_approval.insert(token, U256::from(1000));
        baseline.token_balance.insert(token, U256::from(1000));
        baseline.angstrom_balance.insert(token, U256::from(1000));
        accounts.last_known_state.insert(user, baseline);

        // Add pending actions
        let action1 =
            create_test_pending_action(token, U256::from(100), U256::from(50), U256::from(100), 1);
        let action2 =
            create_test_pending_action(token, U256::from(200), U256::from(100), U256::from(200), 2);

        accounts.insert_pending_user_action(user, action1);
        accounts.insert_pending_user_action(user, action2);

        // Test live state for different nonces
        let live_state = accounts
            .try_fetch_live_pending_state(user, token, RespendAvoidanceMethod::Nonce(1))
            .unwrap();

        assert_eq!(live_state.approval, U256::from(900)); // 1000 - 100
        assert_eq!(live_state.balance, U256::from(900)); // 1000 - 100
        assert_eq!(live_state.angstrom_balance, U256::from(950)); // 1000 - 50

        let live_state = accounts
            .try_fetch_live_pending_state(user, token, RespendAvoidanceMethod::Nonce(2))
            .unwrap();

        assert_eq!(live_state.approval, U256::from(700)); // 1000 - 100 - 200
        assert_eq!(live_state.balance, U256::from(700)); // 1000 - 100 - 200
        assert_eq!(live_state.angstrom_balance, U256::from(850)); // 1000 - 50 -
                                                                  // 100
    }

    #[test]
    fn test_insert_pending_user_action_with_invalidation() {
        let accounts = setup_test_accounts();
        let user = address!("1234567890123456789012345678901234567890");
        let token = address!("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef");

        // Set up initial state with limited balance
        let mut baseline = BaselineState::default();
        baseline.token_approval.insert(token, U256::from(1000));
        baseline.token_balance.insert(token, U256::from(1000));
        baseline.angstrom_balance.insert(token, U256::from(1000));
        accounts.last_known_state.insert(user, baseline);

        // Add action that consumes most of the balance
        let action1 =
            create_test_pending_action(token, U256::from(900), U256::from(0), U256::from(900), 1);
        accounts.insert_pending_user_action(user, action1);

        // Add action that would overflow the balance
        let action2 =
            create_test_pending_action(token, U256::from(200), U256::from(0), U256::from(200), 2);
        let invalidated = accounts.insert_pending_user_action(user, action2.clone());

        // The second action should be invalidated due to insufficient balance
        assert!(invalidated.contains(&action2.order_hash));
    }

    #[test]
    fn test_new_block_with_empty_state() {
        let accounts = setup_test_accounts();
        accounts.new_block(vec![], vec![]);
        assert!(accounts.pending_actions.is_empty());
        assert!(accounts.last_known_state.is_empty());
    }

    #[test]
    fn test_cancel_nonexistent_user() {
        let accounts = setup_test_accounts();
        let user = address!("1234567890123456789012345678901234567890");
        assert!(!accounts.cancel_order(&user, &B256::random()));
    }

    #[test]
    fn test_respend_conflicts_with_block_avoidance() {
        let accounts = setup_test_accounts();
        let user = address!("1234567890123456789012345678901234567890");
        let token = address!("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef");

        let action =
            create_test_pending_action(token, U256::from(100), U256::from(0), U256::from(100), 1);
        accounts.insert_pending_user_action(user, action);

        let conflicts = accounts.respend_conflicts(user, RespendAvoidanceMethod::Block(1));
        assert!(conflicts.is_empty());
    }

    #[test]
    fn test_live_state_with_multiple_tokens() {
        let accounts = setup_test_accounts();
        let user = address!("1234567890123456789012345678901234567890");
        let token1 = address!("1111111111111111111111111111111111111111");
        let token2 = address!("2222222222222222222222222222222222222222");

        // Set up initial state for multiple tokens
        let mut baseline = BaselineState::default();
        baseline.token_approval.insert(token1, U256::from(1000));
        baseline.token_balance.insert(token1, U256::from(1000));
        baseline.angstrom_balance.insert(token1, U256::from(1000));
        baseline.token_approval.insert(token2, U256::from(2000));
        baseline.token_balance.insert(token2, U256::from(2000));
        baseline.angstrom_balance.insert(token2, U256::from(2000));
        accounts.last_known_state.insert(user, baseline);

        // Add pending actions for different tokens
        let action1 =
            create_test_pending_action(token1, U256::from(100), U256::from(50), U256::from(100), 1);
        let action2 = create_test_pending_action(
            token2,
            U256::from(200),
            U256::from(100),
            U256::from(200),
            1
        );

        accounts.insert_pending_user_action(user, action1);
        accounts.insert_pending_user_action(user, action2);

        // Check live state for token1
        let live_state = accounts
            .try_fetch_live_pending_state(user, token1, RespendAvoidanceMethod::Nonce(1))
            .unwrap();
        assert_eq!(live_state.approval, U256::from(900));
        assert_eq!(live_state.balance, U256::from(900));
        assert_eq!(live_state.angstrom_balance, U256::from(950));

        // Check live state for token2
        let live_state = accounts
            .try_fetch_live_pending_state(user, token2, RespendAvoidanceMethod::Nonce(1))
            .unwrap();
        assert_eq!(live_state.approval, U256::from(1800));
        assert_eq!(live_state.balance, U256::from(1800));
        assert_eq!(live_state.angstrom_balance, U256::from(1900));
    }

    #[test]
    fn test_fetch_all_invalidated_orders_with_overflow() {
        let accounts = setup_test_accounts();
        let user = address!("1234567890123456789012345678901234567890");
        let token = address!("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef");

        // Set up initial state with maximum values
        let mut baseline = BaselineState::default();
        baseline.token_approval.insert(token, U256::MAX);
        baseline.token_balance.insert(token, U256::MAX);
        baseline.angstrom_balance.insert(token, U256::MAX);
        accounts.last_known_state.insert(user, baseline);

        // Add action that would cause overflow
        let action = create_test_pending_action(token, U256::MAX, U256::MAX, U256::MAX, 1);
        accounts.insert_pending_user_action(user, action.clone());

        // Add another action that should be invalidated
        let action2 =
            create_test_pending_action(token, U256::from(1), U256::from(1), U256::from(1), 2);
        accounts.insert_pending_user_action(user, action2.clone());

        let invalidated = accounts.fetch_all_invalidated_orders(user, token);
        assert!(invalidated.contains(&action2.order_hash));
    }
}
