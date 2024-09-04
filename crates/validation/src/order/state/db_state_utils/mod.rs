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
