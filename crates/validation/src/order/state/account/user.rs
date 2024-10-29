use std::{
    collections::HashMap,
    sync::{atomic::AtomicU64, Arc}
};

use alloy::primitives::{Address, B256, U256};
use angstrom_types::sol_bindings::{ext::RawPoolOrder, RespendAvoidanceMethod};
use dashmap::DashMap;

use crate::order::state::{db_state_utils::StateFetchUtils, pools::UserOrderPoolInfo};

pub type UserAddress = Address;
pub type TokenAddress = Address;
pub type Amount = U256;

#[derive(Debug, Default)]
pub struct BaselineState {
    token_approval: HashMap<TokenAddress, Amount>,
    token_balance:  HashMap<TokenAddress, Amount>
}

pub struct LiveState {
    pub token:    TokenAddress,
    pub approval: Amount,
    pub balance:  Amount
}

impl LiveState {
    pub fn can_support_order<O: RawPoolOrder>(
        &self,
        order: &O,
        pool_info: &UserOrderPoolInfo
    ) -> Option<PendingUserAction> {
        assert_eq!(order.token_in(), self.token, "incorrect lives state for order");
        let amount_in = U256::from(order.amount_in());
        if self.approval < amount_in || self.balance < amount_in {
            return None
        }

        Some(PendingUserAction {
            order_hash:     order.order_hash(),
            respend:        order.respend_avoidance_strategy(),
            token_address:  pool_info.token,
            token_delta:    amount_in,
            token_approval: amount_in,
            pool_info:      pool_info.clone()
        })
    }
}

/// deltas to be applied to the base user action
#[derive(Clone)]
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

    pub pool_info: UserOrderPoolInfo
}

pub struct UserAccounts {
    /// lets us know what the baseline state is for a user on a given block
    current_block:   Arc<AtomicU64>,
    /// all of a user addresses pending orders.
    pending_actions: Arc<DashMap<UserAddress, Vec<PendingUserAction>>>,

    /// the last updated state of a given user.
    last_known_state: Arc<DashMap<UserAddress, BaselineState>>
}

impl UserAccounts {
    pub fn new(current_block: u64) -> Self {
        Self {
            current_block:    Arc::new(AtomicU64::new(current_block)),
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
        self.pending_actions.retain(|user, pending_orders| {
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
        let balances = utils
            .fetch_balance_for_token(user, token)
            .unwrap_or_default();

        let mut entry = self.last_known_state.entry(user).or_default();
        // override as fresh query
        entry.token_balance.insert(token, balances);
        entry.token_approval.insert(token, approvals);
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
        let baseline = self.last_known_state.get(&user).unwrap();
        let mut baseline_approval = *baseline.token_approval.get(&token).unwrap();
        let mut baseline_balance = *baseline.token_balance.get(&token).unwrap();
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

        // the values returned here are the negative delta compaired to baseline.
        let (pending_approvals, pending_balance) = self
            .pending_actions
            .get(&user)
            .map(|val| {
                val.iter()
                    .filter(|state| state.token_address == token)
                    .take_while(|state| {
                        state.respend.get_ord_for_pending_orders()
                            <= respend.get_ord_for_pending_orders()
                    })
                    .fold((Amount::default(), Amount::default()), |(mut approvals, mut bal), x| {
                        approvals += x.token_approval;
                        bal += x.token_delta;
                        (approvals, bal)
                    })
            })
            .unwrap_or_default();

        let live_approval = baseline_approval.saturating_sub(pending_approvals);
        let live_balance = baseline_balance.saturating_sub(pending_balance);

        Some(LiveState { token, balance: live_balance, approval: live_approval })
    }
}
