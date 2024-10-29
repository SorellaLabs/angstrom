pub mod approvals;
pub mod balances;
pub mod nonces;

use std::{collections::HashMap, fmt::Debug, sync::Arc};

use alloy::primitives::{Address, U256};

use self::{approvals::Approvals, balances::Balances, nonces::Nonces};
use super::config::DataFetcherConfig;

pub const ANGSTROM_CONTRACT: Address = Address::new([0; 20]);

pub trait StateFetchUtils: Clone + Send + Unpin {
    fn is_valid_nonce(&self, user: Address, nonce: u64) -> bool;

    fn fetch_approval_balance_for_token_overrides(
        &self,
        user: Address,
        token: Address,
        overrides: &HashMap<Address, HashMap<U256, U256>>
    ) -> Option<U256>;

    fn fetch_approval_balance_for_token(&self, user: Address, token: Address) -> Option<U256>;

    fn fetch_balance_for_token_overrides(
        &self,
        user: Address,
        token: Address,
        overrides: &HashMap<Address, HashMap<U256, U256>>
    ) -> Option<U256>;

    fn fetch_balance_for_token(&self, user: Address, token: Address) -> Option<U256>;
}

#[derive(Debug)]
pub struct UserAccountDetails {
    pub token:           Address,
    pub token_bals:      U256,
    pub token_approvals: U256,
    pub is_valid_nonce:  bool,
    pub is_valid_pool:   bool,
    pub is_bid:          bool,
    pub pool_id:         usize
}

#[derive(Clone)]
pub struct FetchUtils<DB> {
    pub approvals: Approvals,
    pub balances:  Balances,
    pub nonces:    Nonces,
    pub db:        Arc<DB>
}

impl<DB> StateFetchUtils for FetchUtils<DB>
where
    DB: revm::DatabaseRef + Clone + Sync + Send,
    <DB as revm::DatabaseRef>::Error: Sync + Send + 'static + Debug
{
    fn is_valid_nonce(&self, user: Address, nonce: u64) -> bool {
        let db = self.db.clone();
        self.nonces.is_valid_nonce(user, nonce, db)
    }

    fn fetch_approval_balance_for_token_overrides(
        &self,
        user: Address,
        token: Address,
        overrides: &HashMap<Address, HashMap<U256, U256>>
    ) -> Option<U256> {
        let db = self.db.clone();
        self.approvals
            .fetch_approval_balance_for_token_overrides(user, token, db, overrides)
    }

    fn fetch_approval_balance_for_token(&self, user: Address, token: Address) -> Option<U256> {
        self.approvals
            .fetch_approval_balance_for_token(user, token, &self.db)
    }

    fn fetch_balance_for_token_overrides(
        &self,
        user: Address,
        token: Address,
        overrides: &HashMap<Address, HashMap<U256, U256>>
    ) -> Option<U256> {
        let db = self.db.clone();
        self.balances
            .fetch_balance_for_token_overrides(user, token, db, overrides)
    }

    fn fetch_balance_for_token(&self, user: Address, token: Address) -> Option<U256> {
        self.balances.fetch_balance_for_token(user, token, &self.db)
    }
}

impl<DB> FetchUtils<DB> {
    pub fn new(config: DataFetcherConfig, db: Arc<DB>) -> Self {
        Self {
            approvals: Approvals::new(
                config
                    .approvals
                    .into_iter()
                    .map(|app| (app.token, app))
                    .collect()
            ),
            balances: Balances::new(
                config
                    .balances
                    .into_iter()
                    .map(|bal| (bal.token, bal))
                    .collect()
            ),
            nonces: Nonces,
            db
        }
    }
}

#[cfg(test)]
pub mod test_fetching {
    use std::collections::{HashMap, HashSet};

    use alloy::primitives::U256;
    use dashmap::DashMap;

    use super::{StateFetchUtils, *};

    #[derive(Debug, Clone, Default)]
    pub struct MockFetch {
        balance_values:  DashMap<Address, HashMap<Address, U256>>,
        approval_values: DashMap<Address, HashMap<Address, U256>>,
        used_nonces:     DashMap<Address, HashSet<u64>>
    }

    impl MockFetch {
        pub fn set_balance_for_user(&self, user: Address, token: Address, value: U256) {
            self.balance_values
                .entry(user)
                .or_default()
                .insert(token, value);
        }

        pub fn set_approval_for_user(&self, user: Address, token: Address, value: U256) {
            self.approval_values
                .entry(user)
                .or_default()
                .insert(token, value);
        }

        pub fn set_used_nonces(&self, user: Address, nonces: HashSet<u64>) {
            self.used_nonces.entry(user).or_default().extend(nonces);
        }
    }

    impl StateFetchUtils for MockFetch {
        fn is_valid_nonce(&self, user: alloy::primitives::Address, nonce: u64) -> bool {
            self.used_nonces
                .get(&user)
                .map(|v| !v.value().contains(&nonce))
                .unwrap_or(true)
        }

        fn fetch_approval_balance_for_token_overrides(
            &self,
            _: Address,
            _: Address,
            _: &HashMap<Address, HashMap<U256, U256>>
        ) -> Option<U256> {
            todo!("not implemented for mocker")
        }

        fn fetch_approval_balance_for_token(&self, user: Address, token: Address) -> Option<U256> {
            self.approval_values
                .get(&user)
                .and_then(|inner| inner.value().get(&token).cloned())
        }

        fn fetch_balance_for_token_overrides(
            &self,
            _: Address,
            _: Address,
            _: &HashMap<Address, HashMap<U256, U256>>
        ) -> Option<U256> {
            todo!("not implemented for mocker")
        }

        fn fetch_balance_for_token(&self, user: Address, token: Address) -> Option<U256> {
            self.balance_values
                .get(&user)
                .and_then(|inner| inner.value().get(&token).cloned())
        }
    }
}
