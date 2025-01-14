pub mod approvals;
pub mod balances;
pub mod nonces;

mod finders;

use std::{collections::HashMap, fmt::Debug, sync::Arc};

use alloy::primitives::{Address, U256};
use angstrom_metrics::validation::ValidationMetrics;

use self::{approvals::Approvals, balances::Balances, nonces::Nonces};

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

    fn fetch_balance_for_token(&self, user: Address, token: Address) -> U256;

    fn fetch_token_balance_in_angstrom(&self, user: Address, token: Address) -> U256;
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
    pub db:        Arc<DB>,
    metrics:       ValidationMetrics
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
        self.metrics.loading_approvals(|| {
            self.approvals
                .fetch_approval_balance_for_token_overrides(user, token, db, overrides)
        })
    }

    fn fetch_approval_balance_for_token(&self, user: Address, token: Address) -> Option<U256> {
        self.metrics.loading_approvals(|| {
            self.approvals
                .fetch_approval_balance_for_token(user, token, &self.db)
        })
    }

    fn fetch_token_balance_in_angstrom(&self, user: Address, token: Address) -> U256 {
        self.metrics.loading_balances(|| {
            self.balances
                .fetch_balance_in_angstrom(user, token, &self.db)
        })
    }

    fn fetch_balance_for_token_overrides(
        &self,
        user: Address,
        token: Address,
        overrides: &HashMap<Address, HashMap<U256, U256>>
    ) -> Option<U256> {
        let db = self.db.clone();
        self.metrics.loading_balances(|| {
            self.balances
                .fetch_balance_for_token_overrides(user, token, db, overrides)
        })
    }

    fn fetch_balance_for_token(&self, user: Address, token: Address) -> U256 {
        self.metrics
            .loading_balances(|| self.balances.fetch_balance_for_token(user, token, &self.db))
    }
}

impl<DB: revm::DatabaseRef> FetchUtils<DB> {
    pub fn new(angstrom_address: Address, db: Arc<DB>) -> Self {
        Self {
            approvals: Approvals::new(angstrom_address),
            balances: Balances::new(angstrom_address),
            nonces: Nonces::new(angstrom_address),
            db,
            metrics: ValidationMetrics::new()
        }
    }
}

#[derive(Clone)]
pub struct AutoMaxFetchUtils;

impl StateFetchUtils for AutoMaxFetchUtils {
    fn is_valid_nonce(&self, _: Address, _: u64) -> bool {
        true
    }

    fn fetch_approval_balance_for_token_overrides(
        &self,
        _: Address,
        _: Address,
        _: &HashMap<Address, HashMap<U256, U256>>
    ) -> Option<U256> {
        Some(U256::MAX)
    }

    fn fetch_approval_balance_for_token(&self, _: Address, _: Address) -> Option<U256> {
        Some(U256::MAX)
    }

    fn fetch_balance_for_token_overrides(
        &self,
        _: Address,
        _: Address,
        _: &HashMap<Address, HashMap<U256, U256>>
    ) -> Option<U256> {
        Some(U256::MAX)
    }

    fn fetch_balance_for_token(&self, _: Address, _: Address) -> U256 {
        U256::MAX
    }

    fn fetch_token_balance_in_angstrom(&self, _: Address, _: Address) -> U256 {
        U256::MAX
    }
}

#[cfg(test)]
pub mod test_fetching {
    use std::collections::{HashMap, HashSet};

    use alloy::primitives::{address, U256};
    use dashmap::DashMap;

    use super::{StateFetchUtils, *};

    #[derive(Debug, Clone, Default)]
    pub struct MockFetch {
        balance_values:  DashMap<Address, HashMap<Address, U256>>,
        angstrom_values: DashMap<Address, HashMap<Address, U256>>,
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

        fn fetch_balance_for_token(&self, user: Address, token: Address) -> U256 {
            self.balance_values
                .get(&user)
                .and_then(|inner| inner.value().get(&token).cloned())
                .unwrap_or_default()
        }

        fn fetch_token_balance_in_angstrom(&self, user: Address, token: Address) -> U256 {
            self.angstrom_values
                .get(&user)
                .and_then(|inner| inner.value().get(&token).cloned())
                .unwrap_or_default()
        }
    }

    fn setup_mock_fetch() -> MockFetch {
        MockFetch::default()
    }

    #[test]
    fn test_balance_operations() {
        let mock = setup_mock_fetch();
        let user = address!("1234567890123456789012345678901234567890");
        let token = address!("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef");
        let balance = U256::from(1000);

        // Test setting and fetching balance
        mock.set_balance_for_user(user, token, balance);
        assert_eq!(mock.fetch_balance_for_token(user, token), balance);

        // Test non-existent balance returns zero
        let other_token = address!("beefdeadbeefdeadbeefdeadbeefdeadbeefdead");
        assert_eq!(mock.fetch_balance_for_token(user, other_token), U256::ZERO);
    }

    #[test]
    fn test_approval_operations() {
        let mock = setup_mock_fetch();
        let user = address!("1234567890123456789012345678901234567890");
        let token = address!("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef");
        let approval = U256::from(500);

        // Test setting and fetching approval
        mock.set_approval_for_user(user, token, approval);
        assert_eq!(mock.fetch_approval_balance_for_token(user, token), Some(approval));

        // Test non-existent approval returns None
        let other_token = address!("beefdeadbeefdeadbeefdeadbeefdeadbeefdead");
        assert_eq!(mock.fetch_approval_balance_for_token(user, other_token), None);
    }

    #[test]
    fn test_nonce_validation() {
        let mock = setup_mock_fetch();
        let user = address!("1234567890123456789012345678901234567890");

        // Test unused nonce is valid
        assert!(mock.is_valid_nonce(user, 1));

        // Set used nonces
        let mut used_nonces = HashSet::new();
        used_nonces.insert(1);
        used_nonces.insert(2);
        mock.set_used_nonces(user, used_nonces);

        // Test used nonces are invalid
        assert!(!mock.is_valid_nonce(user, 1));
        assert!(!mock.is_valid_nonce(user, 2));

        // Test unused nonce is still valid
        assert!(mock.is_valid_nonce(user, 3));
    }

    #[test]
    fn test_angstrom_balance() {
        let mock = setup_mock_fetch();
        let user = address!("1234567890123456789012345678901234567890");
        let token = address!("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef");

        // Test default angstrom balance is zero
        assert_eq!(mock.fetch_token_balance_in_angstrom(user, token), U256::ZERO);

        // We can't directly set angstrom values as there's no public method,
        // but we can verify the default behavior
        let other_token = address!("beefdeadbeefdeadbeefdeadbeefdeadbeefdead");
        assert_eq!(mock.fetch_token_balance_in_angstrom(user, other_token), U256::ZERO);
    }

    #[test]
    #[should_panic]
    fn test_overrides_not_implemented() {
        let mock = setup_mock_fetch();
        let user = address!("1234567890123456789012345678901234567890");
        let token = address!("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef");
        let overrides = HashMap::new();

        // Verify that override methods panic with todo!()
        mock.fetch_approval_balance_for_token_overrides(user, token, &overrides);
        mock.fetch_balance_for_token_overrides(user, token, &overrides);
    }
}
