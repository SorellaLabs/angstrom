use std::{
    collections::{HashMap, HashSet},
    sync::{atomic::AtomicU64, Arc}
};

use alloy_primitives::Address;
use dashmap::DashMap;
use parking_lot::RwLock;
use reth_primitives::{B256, U256};

use crate::{order::state::db_state_utils::FetchUtils, BlockStateProviderFactory, RevmLRU};

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

    pub is_valid_nonce: bool,
    pub is_valid_pool:  bool,
    pub is_bid:         bool,
    pub pool_id:        usize
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

    fn load_state_for<DB: Send + BlockStateProviderFactory>(
        &mut self,
        user: UserAddress,
        token: TokenAddress,
        utils: &FetchUtils,
        db: Arc<RevmLRU<DB>>
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
