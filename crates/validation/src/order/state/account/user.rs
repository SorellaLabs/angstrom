use std::{
    collections::{HashMap, HashSet},
    sync::{atomic::AtomicU64, Arc}
};

use alloy_primitives::Address;
use angstrom_types::sol_bindings::grouped_orders::{PoolOrder, RawPoolOrder};
use dashmap::DashMap;
use parking_lot::RwLock;
use reth_primitives::{B256, U256};

use crate::{
    order::state::{
        db_state_utils::FetchUtils, pools::UserOrderPoolInfo, AssetIndexToAddressWrapper
    },
    BlockStateProviderFactory, RevmLRU
};

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
        order: &AssetIndexToAddressWrapper<O>,
        pool_info: &UserOrderPoolInfo
    ) -> Option<PendingUserAction> {
        assert_eq!(order.token_in(), self.token, "incorrect lives state for order");
        let amount_in = U256::from(order.amount_in());
        if self.approval < amount_in || self.balance < amount_in {
            return None
        }
        Some(PendingUserAction {
            order_hash:     order.hash(),
            nonce:          order.nonce(),
            token_address:  pool_info.token,
            token_delta:    amount_in,
            token_approval: amount_in,
            pool_info:      pool_info.clone()
        })
    }
}

/// deltas to be applied to the base user action
pub struct PendingUserAction {
    /// hash of order
    pub order_hash:     B256,
    pub nonce:          U256,
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
    current_block:   AtomicU64,
    /// all of a user addresses pending orders.
    pending_actions: Arc<DashMap<UserAddress, Vec<PendingUserAction>>>,

    /// the last updated state of a given user.
    last_known_state: Arc<DashMap<UserAddress, BaselineState>>
}

impl UserAccounts {
    pub fn current_block(&self) -> u64 {
        self.current_block.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// returns true if the order cancel has been processed successfully
    pub fn cancel_order(&mut self, user: &UserAddress, order_hash: B256) -> bool {
        let Some(mut inner_orders) = self.pending_actions.get_mut(user) else { return false };
        let mut res = false;
        inner_orders.retain(|o| {
            let matches = o.order_hash != order_hash;
            res |= !matches;
            matches
        });

        res
    }

    pub fn has_nonce_conflict(&self, user: UserAddress, nonce: U256) -> bool {
        self.pending_actions
            .get(&user)
            .map(|v| {
                v.value()
                    .iter()
                    .find(|pending_order| pending_order.nonce == nonce)
                    .is_some()
            })
            .unwrap_or_default()
    }

    pub fn get_live_state_for_order<DB: Send + BlockStateProviderFactory>(
        &self,
        user: UserAddress,
        token: TokenAddress,
        nonce: U256,
        utils: &FetchUtils,
        db: &RevmLRU<DB>
    ) -> LiveState {
        self.try_fetch_live_pending_state(user, token, nonce)
            .unwrap_or_else(|| {
                self.load_state_for(user, token, utils, db);
                self.try_fetch_live_pending_state(user, token, nonce)
                    .expect(
                        "after loading state for a address, the state wasn't found. this should \
                         be impossible"
                    )
            })
    }

    fn load_state_for<DB: Send + BlockStateProviderFactory>(
        &self,
        user: UserAddress,
        token: TokenAddress,
        utils: &FetchUtils,
        db: &RevmLRU<DB>
    ) {
        let approvals = utils
            .approvals
            .fetch_approval_balance_for_token(user, token, db.clone())
            .unwrap_or_default();
        let balances = utils
            .balances
            .fetch_balance_for_token(user, token, db)
            .unwrap_or_default();

        let mut entry = self.last_known_state.entry(user).or_default();
        // override as fresh query
        entry.token_balance.insert(token, balances);
        entry.token_approval.insert(token, approvals);
    }

    /// for the given user and token_in, and nonce, will return none
    /// if there is no baseline information for the given user
    /// account.
    fn try_fetch_live_pending_state(
        &self,
        user: UserAddress,
        token: TokenAddress,
        nonce: U256
    ) -> Option<LiveState> {
        let baseline = self.last_known_state.get(&user)?;
        let mut baseline_approval = baseline.token_approval.get(&token)?.clone();
        let mut baseline_balance = baseline.token_balance.get(&token)?.clone();

        // the values returned here are the negative delta compaired to baseline.
        let (pending_approvals, pending_balance) = self
            .pending_actions
            .get(&user)
            .map(|val| {
                val.iter()
                    .filter(|state| state.token_address == token)
                    .take_while(|state| state.nonce < nonce)
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
