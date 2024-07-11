pub mod angstrom_pools;
pub mod approvals;
pub mod balances;
pub mod nonces;

use std::{cmp::Ordering, collections::HashMap, sync::Arc};

use alloy_primitives::{keccak256, Address, Bytes, FixedBytes, B256, U256};
use angstrom_types::sol_bindings::grouped_orders::{
    AllOrders, GroupedComposableOrder, GroupedVanillaOrder
};
use reth_primitives::revm_primitives::{Env, TransactTo, TxEnv};
use reth_revm::EvmBuilder;
use revm::{db::WrapDatabaseRef, interpreter::opcode, Database, Inspector};

use self::{
    angstrom_pools::AngstromPools, approvals::Approvals, balances::Balances, nonces::Nonces
};
use super::config::ValidationConfig;
use crate::common::lru_db::{BlockStateProviderFactory, RevmLRU};

pub const ANGSTROM_CONTRACT: Address = Address::new([0; 20]);

#[derive(Debug)]
pub struct UserAccountDetails {
    pub token_bals:      (Address, U256),
    pub token_approvals: (Address, U256),
    pub is_valid_nonce:  bool,
    pub is_valid_pool:   bool,
    pub is_bid:          bool,
    pub pool_id:         usize
}

pub struct Upkeepers {
    approvals: Approvals,
    balances:  Balances,
    pools:     AngstromPools,
    nonces:    Nonces
}

impl Upkeepers {
    pub fn new(config: ValidationConfig) -> Self {
        Self {
            approvals: Approvals::new(
                config
                    .approvals
                    .into_iter()
                    .map(|app| (app.token, app))
                    .collect()
            ),
            pools:     AngstromPools::new(
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

    pub fn verify_order<DB: Send + BlockStateProviderFactory>(
        &self,
        order: AllOrders,
        db: Arc<RevmLRU<DB>>
    ) -> (UserAccountDetails, AllOrders) {
        let is_valid_nonce =
            self.nonces
                .is_valid_nonce(order.from(), order.nonce().to(), db.clone());

        let (is_valid_pool, is_bid, pool_id) = self
            .pools
            .order_info(order.token_in(), order.token_out())
            .map(|(bid, pool_id)| (true, bid, pool_id))
            .unwrap_or_default();

        let approvals = self
            .approvals
            .fetch_approval_balance_for_token(order.from(), order.token_in(), db.clone())
            .unwrap_or_default();

        let balances = self
            .balances
            .fetch_balance_for_token(order.from(), order.token_in(), db.clone())
            .unwrap_or_default();

        (
            UserAccountDetails {
                pool_id,
                is_bid,
                is_valid_nonce,
                token_bals: (order.token_in(), balances),
                is_valid_pool,
                token_approvals: (order.token_in(), approvals)
            },
            order
        )
    }

    pub fn verify_composable_order<DB: Send + BlockStateProviderFactory>(
        &self,
        order: GroupedComposableOrder,
        db: Arc<RevmLRU<DB>>,
        overrides: &HashMap<Address, HashMap<U256, U256>>
    ) -> (UserAccountDetails, GroupedComposableOrder) {
        let is_valid_nonce =
            self.nonces
                .is_valid_nonce(order.from(), order.nonce().to(), db.clone());

        let (is_valid_pool, is_bid, pool_id) = self
            .pools
            .order_info(order.token_in(), order.token_out())
            .map(|(bid, pool_id)| (true, bid, pool_id))
            .unwrap_or_default();

        let approvals = self
            .approvals
            .fetch_approval_balance_for_token_overrides(
                order.from(),
                order.token_in(),
                db.clone(),
                overrides
            )
            .unwrap_or_default();

        let balances = self
            .balances
            .fetch_balance_for_token_overrides(
                order.from(),
                order.token_in(),
                db.clone(),
                overrides
            )
            .unwrap_or_default();

        (
            UserAccountDetails {
                pool_id,
                is_bid,
                is_valid_nonce,
                token_bals: (order.token_in(), balances),
                is_valid_pool,
                token_approvals: (order.token_in(), approvals)
            },
            order
        )
    }
}

// pub fn find_storage_slot<DB>(
//     call_data: Bytes,
//     wanted_address: Address,
//     db: RevmLRU<DB>
// ) -> anyhow::Result<U256>
// where
//     DB: BlockStateProviderFactory + Send + Sync + Clone + 'static
// {
//     let prob_address = Address::new([0, 0, 0, 0, 0, 0, 0, 0, 0, 11, 0, 0, 0,
// 0, 0, 64, 0, 0, 0, 0]);
//
//     let mut tx_env = TxEnv::default();
//     tx_env.transact_to = TransactTo::Call(wanted_address);
//     tx_env.data = call_data.clone();
//     tx_env.caller = prob_address;
//
//     let mut evm = EvmBuilder::default()
//         .with_db(db.clone())
//         .with_tx_env(tx_env.clone())
//         .build();
//
//     let res =
// U256::from_be_slice(&evm.transact().unwrap().result.output().unwrap().0);
//
//     let one = U256::from(1);
//     let prob_value = if res == U256::MAX { res - one } else { res + one };
//
//     for i in 0..100 {
//         let mut user_addr_encoded = prob_address.to_vec();
//         user_addr_encoded.extend(U256::from(i).to_be_bytes::<32>().to_vec());
//
//         let user_slot = U256::from_be_bytes(*keccak256(user_addr_encoded));
//         let mut slot = HashMap::new();
//         slot.insert(user_slot, prob_value);
//         let mut overrides = HashMap::new();
//         overrides.insert(wanted_address, slot);
//         let mut db = db.clone();
//         db.set_state_overrides(overrides);
//
//         let mut tx_env = TxEnv::default();
//         tx_env.transact_to = TransactTo::Call(wanted_address);
//         tx_env.data = call_data.clone();
//         tx_env.caller = prob_address;
//
//         let mut evm = EvmBuilder::default()
//             .with_db(db)
//             .with_tx_env(tx_env.clone())
//             .build();
//
//         let res =
// U256::from_be_slice(&evm.transact().unwrap().result.output().unwrap().0);
//
//         if res == prob_value {
//             return Ok(U256::from(i))
//         }
//     }
//
//     anyhow::bail!("should never be reached");
// }
