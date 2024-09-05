pub mod approvals;
pub mod balances;
pub mod nonces;

use std::{cmp::Ordering, collections::HashMap, sync::Arc};

use alloy_primitives::{keccak256, Address, Bytes, FixedBytes, B256, U256};
use angstrom_types::sol_bindings::grouped_orders::{PoolOrder, RawPoolOrder};
use reth_primitives::revm_primitives::{Env, TransactTo, TxEnv};
use reth_revm::EvmBuilder;
use revm::{db::WrapDatabaseRef, interpreter::opcode, Database, Inspector};

use self::{approvals::Approvals, balances::Balances, nonces::Nonces};
use super::config::ValidationConfig;
use crate::common::lru_db::{BlockStateProviderFactory, RevmLRU};

pub const ANGSTROM_CONTRACT: Address = Address::new([0; 20]);

pub trait StateFetchUtils: Clone + Send + Unpin {
    fn is_valid_nonce<DB: BlockStateProviderFactory>(
        &self,
        user: Address,
        nonce: u64,
        db: Arc<RevmLRU<DB>>
    ) -> bool;

    fn fetch_approval_balance_for_token_overrides<DB: BlockStateProviderFactory>(
        &self,
        user: Address,
        token: Address,
        db: Arc<RevmLRU<DB>>,
        overrides: &HashMap<Address, HashMap<U256, U256>>
    ) -> Option<U256>;

    fn fetch_approval_balance_for_token<DB: BlockStateProviderFactory>(
        &self,
        user: Address,
        token: Address,
        db: &RevmLRU<DB>
    ) -> Option<U256>;

    fn fetch_balance_for_token_overrides<DB: BlockStateProviderFactory>(
        &self,
        user: Address,
        token: Address,
        db: Arc<RevmLRU<DB>>,
        overrides: &HashMap<Address, HashMap<U256, U256>>
    ) -> Option<U256>;

    fn fetch_balance_for_token<DB: BlockStateProviderFactory>(
        &self,
        user: Address,
        token: Address,
        db: &RevmLRU<DB>
    ) -> Option<U256>;
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
pub struct FetchUtils {
    pub approvals: Approvals,
    pub balances:  Balances,
    pub nonces:    Nonces
}

impl StateFetchUtils for FetchUtils {
    fn is_valid_nonce<DB: BlockStateProviderFactory>(
        &self,
        user: Address,
        nonce: u64,
        db: Arc<RevmLRU<DB>>
    ) -> bool {
        self.nonces.is_valid_nonce(user, nonce, db)
    }

    fn fetch_balance_for_token<DB: BlockStateProviderFactory>(
        &self,
        user: Address,
        token: Address,
        db: &RevmLRU<DB>
    ) -> Option<U256> {
        self.balances.fetch_balance_for_token(user, token, db)
    }

    fn fetch_approval_balance_for_token<DB: BlockStateProviderFactory>(
        &self,
        user: Address,
        token: Address,
        db: &RevmLRU<DB>
    ) -> Option<U256> {
        self.approvals
            .fetch_approval_balance_for_token(user, token, db)
    }

    fn fetch_balance_for_token_overrides<DB: BlockStateProviderFactory>(
        &self,
        user: Address,
        token: Address,
        db: Arc<RevmLRU<DB>>,
        overrides: &HashMap<Address, HashMap<U256, U256>>
    ) -> Option<U256> {
        self.balances
            .fetch_balance_for_token_overrides(user, token, db, overrides)
    }

    fn fetch_approval_balance_for_token_overrides<DB: BlockStateProviderFactory>(
        &self,
        user: Address,
        token: Address,
        db: Arc<RevmLRU<DB>>,
        overrides: &HashMap<Address, HashMap<U256, U256>>
    ) -> Option<U256> {
        self.approvals
            .fetch_approval_balance_for_token_overrides(user, token, db, overrides)
    }
}

impl FetchUtils {
    pub fn new(config: ValidationConfig) -> Self {
        Self {
            approvals: Approvals::new(
                config
                    .approvals
                    .into_iter()
                    .map(|app| (app.token, app))
                    .collect()
            ),
            balances:  Balances::new(
                config
                    .balances
                    .into_iter()
                    .map(|bal| (bal.token, bal))
                    .collect()
            ),
            nonces:    Nonces
        }
    }
}

#[cfg(test)]
pub mod test_fetching {
    use std::collections::{HashMap, HashSet};

    use alloy_primitives::U256;
    use dashmap::DashMap;

    use super::{StateFetchUtils, *};

    #[derive(Debug, Clone)]
    pub struct MockFetch {
        balance_values:  DashMap<Address, HashMap<Address, U256>>,
        approval_values: DashMap<Address, HashMap<Address, U256>>,
        used_nonces:     DashMap<Address, HashSet<u64>>
    }

    impl MockFetch {
        pub fn set_balance_for_user(&self, user: Address, token: Address, value: U256) {
            self.balance_values
                .entry(user)
                .or_insert(HashMap::default())
                .insert(token, value);
        }

        pub fn set_approval_for_user(&self, user: Address, token: Address, value: U256) {
            self.approval_values
                .entry(user)
                .or_insert(HashMap::default())
                .insert(token, value);
        }

        pub fn set_used_nonces(&self, user: Address, nonces: HashSet<u64>) {
            self.used_nonces.entry(user).or_default().extend(nonces);
        }
    }

    impl StateFetchUtils for MockFetch {
        fn is_valid_nonce<DB: crate::common::lru_db::BlockStateProviderFactory>(
            &self,
            user: reth_primitives::Address,
            nonce: u64,
            _db: std::sync::Arc<crate::common::lru_db::RevmLRU<DB>>
        ) -> bool {
            self.used_nonces
                .get(&user)
                .map(|v| !v.value().contains(&nonce))
                .unwrap_or_default()
        }

        fn fetch_balance_for_token<DB: BlockStateProviderFactory>(
            &self,
            user: Address,
            token: Address,
            _db: &RevmLRU<DB>
        ) -> Option<U256> {
            self.balance_values
                .get(&user)
                .and_then(|inner| inner.value().get(&token).cloned())
        }

        fn fetch_approval_balance_for_token<DB: BlockStateProviderFactory>(
            &self,
            user: Address,
            token: Address,
            db: &RevmLRU<DB>
        ) -> Option<U256> {
            self.approval_values
                .get(&user)
                .and_then(|inner| inner.value().get(&token).cloned())
        }

        fn fetch_balance_for_token_overrides<DB: BlockStateProviderFactory>(
            &self,
            user: Address,
            token: Address,
            db: Arc<RevmLRU<DB>>,
            overrides: &HashMap<Address, HashMap<U256, U256>>
        ) -> Option<U256> {
            todo!("not implemented for mocker")
        }

        fn fetch_approval_balance_for_token_overrides<DB: BlockStateProviderFactory>(
            &self,
            user: Address,
            token: Address,
            db: Arc<RevmLRU<DB>>,
            overrides: &HashMap<Address, HashMap<U256, U256>>
        ) -> Option<U256> {
            todo!("not implemented for mocker")
        }
    }
}
