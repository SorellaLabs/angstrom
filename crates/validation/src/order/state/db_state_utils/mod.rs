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

pub struct FetchUtils {
    pub approvals: Approvals,
    pub balances:  Balances,
    pub nonces:    Nonces
}

impl FetchUtils {
    pub fn new(config: ValidationConfig) -> Self {
        Self {
            approvals:        Approvals::new(
                config
                    .approvals
                    .into_iter()
                    .map(|app| (app.token, app))
                    .collect()
            ),
            pools:            AngstromPools::new(
                config
                    .pools
                    .into_iter()
                    .flat_map(|app| {
                        let direction = app.token0.cmp(&app.token1) == Ordering::Less;
                        [
                            (
                                FixedBytes::concat_const(*app.token0, *app.token1),
                                (direction, app.pool_id)
                            ),
                            (
                                FixedBytes::concat_const(*app.token1, *app.token0),
                                (!direction, app.pool_id)
                            )
                        ]
                    })
                    .collect()
            ),
            balances:         Balances::new(
                config
                    .balances
                    .into_iter()
                    .map(|bal| (bal.token, bal))
                    .collect()
            ),
            nonces:           Nonces,
            asset_to_address: AssetIndexToAddress::default()
        }
    }
}
