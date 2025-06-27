use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    ops::Deref,
    sync::Arc
};

use alloy::primitives::{Address, B256, U256};
use angstrom_types::{
    primitive::{UserAccountVerificationError, UserOrderPoolInfo},
    sol_bindings::{OrderValidationPriority, Ray, RespendAvoidanceMethod, ext::RawPoolOrder}
};
use angstrom_utils::FnResultOption;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};

use crate::order::state::db_state_utils::StateFetchUtils;

pub type UserAddress = Address;
pub type TokenAddress = Address;
pub type Amount = U256;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
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
        pool_info: &UserOrderPoolInfo,
        bid_in_token_in: Option<u128>
    ) -> Result<PendingUserAction, UserAccountVerificationError> {
        assert_eq!(order.token_in(), self.token, "incorrect lives state for order");
        let hash = order.order_hash();

        let amount_in = self.fetch_amount_in(order);

        let (angstrom_delta, token_delta) = if order.use_internal() {
            if self.angstrom_balance < amount_in {
                return Err(UserAccountVerificationError::InsufficientBalance {
                    order_hash: hash,
                    token_in:   self.token,
                    amount:     (amount_in - self.angstrom_balance).to()
                });
            }
            (amount_in, U256::ZERO)
        } else {
            match (self.approval < amount_in, self.balance < amount_in) {
                (true, true) => {
                    return Err(UserAccountVerificationError::InsufficientBoth {
                        order_hash:      hash,
                        token_in:        self.token,
                        amount_balance:  (amount_in - self.balance).to(),
                        amount_approval: (amount_in - self.approval).to()
                    });
                }
                (true, false) => {
                    return Err(UserAccountVerificationError::InsufficientApproval {
                        order_hash: hash,
                        token_in:   self.token,
                        amount:     (amount_in - self.approval).to()
                    });
                }
                (false, true) => {
                    return Err(UserAccountVerificationError::InsufficientBalance {
                        order_hash: hash,
                        token_in:   self.token,
                        amount:     (amount_in - self.balance).to()
                    });
                }
                // is fine
                (false, false) => {}
            };

            (U256::ZERO, amount_in)
        };

        Ok(PendingUserAction {
            order_priority: order.validation_priority(bid_in_token_in),
            token_address: pool_info.token,
            token_delta,
            angstrom_delta,
            token_approval: amount_in,
            pool_info: pool_info.clone()
        })
    }

    /// gas is always applied to t0. when we haven't specified that t0 to be an
    /// exact amount. because of this. there are two case where this can
    /// occur.
    ///
    /// one is an one for zero(bid) swap where we specify exact out.
    ///
    /// one is a zero for 1 swap with an exact out specified
    fn fetch_amount_in<O: RawPoolOrder>(&self, order: &O) -> U256 {
        // if we have a tob that is an ask, we need to add the amounts
        if order.is_tob() && !order.is_bid() {
            return U256::from(order.amount() + order.max_gas_token_0());
        }

        let ray_price = Ray::from(order.limit_price());
        U256::from(match (order.is_bid(), order.exact_in()) {
            (true, false) => {
                ray_price.inverse_quantity(order.amount() + order.max_gas_token_0(), true)
            }
            (false, false) => {
                ray_price.inverse_quantity(order.amount(), true) + order.max_gas_token_0()
            }
            _ => order.amount()
        })
    }
}

/// deltas to be applied to the base user action
/// We need to update this ordering such that we prioritize
/// TOB -> Partial Orders -> Book Orders.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingUserAction {
    pub order_priority: OrderValidationPriority,
    // TODO: easier to properly encode this stuff
    // for each order, there will be two different deltas
    pub token_address:  TokenAddress,
    // although we have deltas for two tokens, we only
    // apply for 1 given the execution of angstrom,
    // all tokens are required before execution.
    pub token_delta:    Amount,
    pub token_approval: Amount,
    // balance spent from angstrom
    pub angstrom_delta: Amount,
    pub pool_info:      UserOrderPoolInfo
}

impl Deref for PendingUserAction {
    type Target = OrderValidationPriority;

    fn deref(&self) -> &Self::Target {
        &self.order_priority
    }
}

/// NB: tob actions have priority over non-tob
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAccounts {
    /// all of a user addresses pending book orders.
    pending_book_actions: Arc<DashMap<UserAddress, Vec<PendingUserAction>>>,
    /// all of a users tob orders. This is tracked separately given
    /// that only 1 order per pool can be valid
    pending_tob_actions:  Arc<DashMap<UserAddress, HashMap<TokenAddress, Vec<PendingUserAction>>>>,

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
            pending_tob_actions:  Arc::new(DashMap::default()),
            pending_book_actions: Arc::new(DashMap::default()),
            last_known_state:     Arc::new(DashMap::default())
        }
    }

    pub fn deep_clone(&self) -> Self {
        let pending_book = Arc::new((*self.pending_book_actions).clone());
        let pending_tob = Arc::new((*self.pending_tob_actions).clone());
        let last_known = Arc::new((*self.last_known_state).clone());

        Self {
            last_known_state:     last_known,
            pending_tob_actions:  pending_tob,
            pending_book_actions: pending_book
        }
    }

    pub fn new_block(&self, users: Vec<Address>, orders: Vec<B256>) {
        // because tob orders are only valid for 1 block.
        // we can always clear
        self.pending_tob_actions.clear();

        // remove all user specific orders
        users.iter().for_each(|user| {
            self.pending_book_actions.remove(user);
            self.last_known_state.remove(user);
        });

        // remove all singular orders
        self.pending_book_actions.retain(|_, pending_orders| {
            pending_orders.retain(|p| !orders.contains(&p.order_hash));
            !pending_orders.is_empty()
        });
    }

    /// returns true if the order cancel has been processed successfully
    pub fn cancel_order(&self, user: &UserAddress, order_hash: &B256) -> bool {
        let mut res = false;
        res |= self.try_cancel_book_order(user, order_hash);

        if !res {
            res |= self.try_cancel_tob_order(user, order_hash)
        }

        res
    }

    fn try_cancel_book_order(&self, user: &UserAddress, order_hash: &B256) -> bool {
        let Some(mut inner_orders) = self.pending_book_actions.get_mut(user) else { return false };
        let mut res = false;

        inner_orders.retain(|o| {
            let matches = &o.order_hash != order_hash;
            res |= !matches;
            matches
        });

        res
    }

    fn try_cancel_tob_order(&self, user: &UserAddress, order_hash: &B256) -> bool {
        let Some(mut inner_orders) = self.pending_tob_actions.get_mut(user) else { return false };
        let mut res = false;

        inner_orders.retain(|_, v| {
            v.retain(|o| {
                let matches = &o.order_hash != order_hash;
                res |= !matches;
                matches
            });

            !v.is_empty()
        });

        res
    }

    pub fn respend_conflicts(
        &self,
        user: UserAddress,
        avoidance: RespendAvoidanceMethod
    ) -> Vec<PendingUserAction> {
        // no respend on tob
        match avoidance {
            nonce @ RespendAvoidanceMethod::Nonce(_) => self
                .pending_book_actions
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
        order_priority: OrderValidationPriority,
        pool_id: B256,
        utils: &S
    ) -> eyre::Result<LiveState> {
        let out = self
            .try_fetch_live_pending_state(user, token, pool_id, order_priority)
            .invert_map_or_else(|| {
                self.load_state_for(user, token, utils)?;
                self.try_fetch_live_pending_state(user, token, pool_id, order_priority)
                    .ok_or(eyre::eyre!(
                        "after loading state for a address, the state wasn't found. this should \
                         be impossible"
                    ))
            })?;

        Ok(out)
    }

    fn load_state_for<S: StateFetchUtils>(
        &self,
        user: UserAddress,
        token: TokenAddress,
        utils: &S
    ) -> eyre::Result<()> {
        let approvals = utils
            .fetch_approval_balance_for_token(user, token)?
            .ok_or_else(|| eyre::eyre!("could not properly find approvals"))?;
        let balances = utils.fetch_balance_for_token(user, token)?;
        let angstrom_balance = utils.fetch_token_balance_in_angstrom(user, token)?;

        let mut entry = self.last_known_state.entry(user).or_default();
        // override as fresh query
        entry.token_balance.insert(token, balances);
        entry.token_approval.insert(token, approvals);
        entry.angstrom_balance.insert(token, angstrom_balance);

        Ok(())
    }

    /// inserts the user action and returns all pending user action hashes that
    /// this, invalidates. i.e higher nonce but no balance / approval
    /// available.
    pub fn insert_pending_user_action(
        &self,
        is_tob: bool,
        user: UserAddress,
        action: PendingUserAction
    ) -> Vec<B256> {
        let token = action.token_address;
        if is_tob {
            // we only want ot insert this if we are the highest tob order for the given
            // pool. when we did the accounting, we verified that we had enough
            // funds
            let mut user_entry = self.pending_tob_actions.entry(user).or_default();
            let token_entry = user_entry.entry(token).or_default();

            token_entry.push(action);
            token_entry.sort_unstable_by(|f, s| s.is_higher_priority(f));

            // tob can invalidate all user orders.
            drop(user_entry);
            self.fetch_all_invalidated_orders(user, token)
        } else {
            let mut entry = self.pending_book_actions.entry(user).or_default();
            let value = entry.value_mut();

            value.push(action);
            // sorts them descending by priority
            value.sort_unstable_by(|f, s| s.is_higher_priority(f));
            drop(entry);

            // iterate through all vales collected the orders that
            self.fetch_all_invalidated_orders(user, token)
        }
    }

    fn fetch_all_invalidated_orders(&self, user: UserAddress, token: TokenAddress) -> Vec<B256> {
        let Some(baseline) = self.last_known_state.get(&user) else { return vec![] };

        let mut baseline_approval = *baseline.token_approval.get(&token).unwrap();
        let mut baseline_balance = *baseline.token_balance.get(&token).unwrap();
        let mut baseline_angstrom_balance = *baseline.angstrom_balance.get(&token).unwrap();
        let mut has_overflowed = false;

        let mut bad = vec![];

        // we want this as
        for pending_state in self.iter_of_tob_and_book_unique_tob(user, token) {
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
        pool_id: B256,
        order_priority: OrderValidationPriority
    ) -> Option<LiveState> {
        let baseline = self.last_known_state.get(&user)?;
        let baseline_approval = *baseline.token_approval.get(&token)?;
        let baseline_balance = *baseline.token_balance.get(&token)?;
        let baseline_angstrom_balance = *baseline.angstrom_balance.get(&token)?;

        // the values returned here are the negative delta compaired to baseline.
        let (pending_approvals_spend, pending_balance_spend, pending_angstrom_balance_spend) = self
            .iter_of_tob_and_book_unique_tob(user, token)
            .filter(|action| {
                // we want to filter out all other tob orders that are on the same pool
                // given that there can only be 1 valid tob per pool.
                !(action.is_tob && order_priority.is_tob && action.pool_info.pool_id == pool_id)
            })
            .take_while(|state| state.is_higher_priority(&order_priority) == Ordering::Greater)
            .fold(
                (Amount::default(), Amount::default(), Amount::default()),
                |(mut approvals_spend, mut balance_spend, mut angstrom_spend), x| {
                    approvals_spend += x.token_approval;
                    balance_spend += x.token_delta;
                    angstrom_spend += x.angstrom_delta;
                    (approvals_spend, balance_spend, angstrom_spend)
                }
            );

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

    fn iter_of_tob_and_book_unique_tob(
        &self,
        user: Address,
        token: TokenAddress
    ) -> impl Iterator<Item = PendingUserAction> + '_ {
        let mut seen_pools = HashSet::new();
        self.iter_of_tob_and_book(user, token)
            .filter(move |f| !f.is_tob || seen_pools.insert(f.pool_info.pool_id))
    }

    fn iter_of_tob_and_book(
        &self,
        user: Address,
        token: TokenAddress
    ) -> impl Iterator<Item = PendingUserAction> + '_ {
        self.pending_tob_actions
            .get(&user)
            .and_then(|a| a.get(&token).cloned())
            .unwrap_or_default()
            .into_iter()
            .chain(
                self.pending_book_actions
                    .get(&user)
                    .map(|a| a.value().clone())
                    .unwrap_or_default()
            )
    }
}

#[cfg(test)]
mod tests {
    use alloy::primitives::address;
    use angstrom_types::primitive::PoolId;

    use super::*;

    fn setup_test_accounts() -> UserAccounts {
        UserAccounts::new()
    }

    fn create_test_pending_action(
        token: TokenAddress,
        token_delta: U256,
        angstrom_delta: U256,
        token_approval: U256,
        nonce: u64,
        is_tob: bool,
        is_partial: bool
    ) -> PendingUserAction {
        PendingUserAction {
            order_priority: OrderValidationPriority {
                order_hash: B256::random(),
                is_tob,
                is_partial,
                respend: RespendAvoidanceMethod::Nonce(nonce),
                tob_bid_amount: 0
            },
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
        let action1 = create_test_pending_action(
            token,
            U256::from(100),
            U256::from(0),
            U256::from(100),
            1,
            false,
            true
        );
        let action2 = create_test_pending_action(
            token,
            U256::from(200),
            U256::from(0),
            U256::from(200),
            2,
            false,
            true
        );

        accounts.insert_pending_user_action(false, user1, action1.clone());
        accounts.insert_pending_user_action(false, user2, action2.clone());

        // Verify actions are present
        assert!(accounts.pending_book_actions.contains_key(&user1));
        assert!(accounts.pending_book_actions.contains_key(&user2));

        // Call new_block to clear specific users and orders
        accounts.new_block(vec![user1], vec![action2.order_hash]);

        // Verify user1's actions are cleared
        assert!(!accounts.pending_book_actions.contains_key(&user1));
        // Verify user2's actions with matching order_hash are cleared
        assert!(
            accounts
                .pending_book_actions
                .get(&user2)
                .is_none_or(|actions| actions.is_empty())
        );
    }

    #[test]
    fn test_cancel_order() {
        let accounts = setup_test_accounts();
        let user = address!("1234567890123456789012345678901234567890");
        let token = address!("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef");

        let action = create_test_pending_action(
            token,
            U256::from(100),
            U256::from(0),
            U256::from(100),
            1,
            false,
            true
        );
        let order_hash = action.order_hash;

        accounts.insert_pending_user_action(false, user, action);

        // Test canceling non-existent order
        assert!(!accounts.cancel_order(&user, &B256::random()));

        // Test canceling existing order
        assert!(accounts.cancel_order(&user, &order_hash));

        // Verify order was removed
        assert!(
            accounts
                .pending_book_actions
                .get(&user)
                .is_none_or(|actions| actions.is_empty())
        );
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
        let action1 = create_test_pending_action(
            token,
            U256::from(100),
            U256::from(0),
            U256::from(100),
            1,
            false,
            true
        );
        let action2 = create_test_pending_action(
            token,
            U256::from(200),
            U256::from(0),
            U256::from(200),
            1,
            false,
            true
        );
        let action3 = create_test_pending_action(
            token,
            U256::from(300),
            U256::from(0),
            U256::from(300),
            2,
            false,
            true
        );

        // Insert actions one by one and verify
        accounts.insert_pending_user_action(false, user, action1.clone());
        let conflicts = accounts.respend_conflicts(user, RespendAvoidanceMethod::Nonce(1));
        assert_eq!(conflicts.len(), 1, "Should find one conflict for nonce 1");

        accounts.insert_pending_user_action(false, user, action2.clone());
        let conflicts = accounts.respend_conflicts(user, RespendAvoidanceMethod::Nonce(1));
        assert_eq!(conflicts.len(), 2, "Should find two conflicts for nonce 1");

        accounts.insert_pending_user_action(false, user, action3.clone());
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
        let action1 = create_test_pending_action(
            token,
            U256::from(100),
            U256::from(50),
            U256::from(100),
            1,
            false,
            true
        );
        let action2 = create_test_pending_action(
            token,
            U256::from(200),
            U256::from(100),
            U256::from(200),
            3,
            false,
            true
        );

        accounts.insert_pending_user_action(false, user, action1);
        accounts.insert_pending_user_action(false, user, action2);

        // Test live state for different nonces
        let live_state = accounts
            .try_fetch_live_pending_state(
                user,
                token,
                PoolId::default(),
                OrderValidationPriority {
                    order_hash:     B256::random(),
                    is_tob:         false,
                    is_partial:     true,
                    tob_bid_amount: 0,
                    respend:        RespendAvoidanceMethod::Nonce(2)
                }
            )
            .unwrap();

        assert_eq!(live_state.approval, U256::from(900)); // 1000 - 100
        assert_eq!(live_state.balance, U256::from(900)); // 1000 - 100
        assert_eq!(live_state.angstrom_balance, U256::from(950)); // 1000 - 50

        let live_state = accounts
            .try_fetch_live_pending_state(
                user,
                token,
                PoolId::default(),
                OrderValidationPriority {
                    order_hash:     B256::random(),
                    is_tob:         false,
                    is_partial:     true,
                    tob_bid_amount: 0,
                    respend:        RespendAvoidanceMethod::Nonce(4)
                }
            )
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
        let action1 = create_test_pending_action(
            token,
            U256::from(900),
            U256::from(0),
            U256::from(900),
            1,
            false,
            true
        );
        accounts.insert_pending_user_action(false, user, action1);

        // Add action that would overflow the balance
        let action2 = create_test_pending_action(
            token,
            U256::from(200),
            U256::from(0),
            U256::from(200),
            2,
            false,
            true
        );
        let invalidated = accounts.insert_pending_user_action(false, user, action2.clone());

        // The second action should be invalidated due to insufficient balance
        assert!(invalidated.contains(&action2.order_hash));
    }

    #[test]
    fn test_new_block_with_empty_state() {
        let accounts = setup_test_accounts();
        accounts.new_block(vec![], vec![]);
        assert!(accounts.pending_book_actions.is_empty());
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

        let action = create_test_pending_action(
            token,
            U256::from(100),
            U256::from(0),
            U256::from(100),
            1,
            false,
            true
        );
        accounts.insert_pending_user_action(false, user, action);

        let conflicts = accounts.respend_conflicts(user, RespendAvoidanceMethod::Block(1));
        assert!(conflicts.is_empty());
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
        let action =
            create_test_pending_action(token, U256::MAX, U256::MAX, U256::MAX, 1, true, true);
        accounts.insert_pending_user_action(false, user, action.clone());

        // Add another action that should be invalidated
        let action2 = create_test_pending_action(
            token,
            U256::from(1),
            U256::from(1),
            U256::from(1),
            2,
            false,
            true
        );
        accounts.insert_pending_user_action(false, user, action2.clone());

        let invalidated = accounts.fetch_all_invalidated_orders(user, token);
        assert!(invalidated.contains(&action2.order_hash));
    }
}
