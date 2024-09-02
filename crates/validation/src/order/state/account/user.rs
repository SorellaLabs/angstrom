use std::collections::HashMap;

use alloy_primitives::Address;
use reth_primitives::{B256, U256};

pub type UserAddress = Address;
pub type TokenAddress = Address;
pub type Amount = U256;

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
pub struct PendingUserActions {
    /// hash of order
    order_hash:     B256,
    nonce:          U256,
    // for each order, there will be two different deltas
    token_address:  TokenAddress,
    // although we have deltas for two tokens, we only
    // apply for 1 given the execution of angstrom,
    // all tokens are required before execution.
    token_delta:    Amount,
    token_approval: Amount
}

pub struct UserAccounts {
    /// all of a user addresses pending orders.
    pending_actions:  HashMap<UserAddress, Vec<PendingUserActions>>,
    /// the last updated state of a given user.
    last_known_state: HashMap<UserAddress, BaselineState>
}

impl UserAccounts {
    /// returns true if the order cancel has been processed successfully
    pub fn cancel_order(&mut self, user: &UserAddress, order_hash: B256) -> bool {
        let Some(inner_orders) = self.pending_actions.get_mut(user) else { return false };
        let mut res = false;
        inner_orders.retain(|o| {
            let matches = o.order_hash != order_hash;
            res |= !matches;
            matches
        });

        res
    }

    /// for the given user and token_in, and nonce, will return none
    /// if there is no baseline information for the given user
    /// account.
    pub fn fetch_live_pending_state(
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
