//! keeps track of account state for orders
use std::sync::Arc;

use user::UserAccounts;

use super::fetch_utils::FetchUtils;

pub mod user;

/// For all active users, stores critical information needed in order to
/// properly verify there actions
pub struct UserAccountTracker<DB> {
    reth_db:       Arc<DB>,
    /// keeps track of all user accounts 
    user_accounts: UserAccounts,
    /// utils for fetching the required data to verify
    /// a order.
    fetch_utils:   FetchUtils
}
