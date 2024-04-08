pub mod angstrom_pools;
pub mod angstrom_tokens;
pub mod approvals;
pub mod balances;
pub mod nonces;

use std::{collections::HashMap, sync::Arc};

use alloy_primitives::{keccak256, Address, Bytes, B256, U256};
use angstrom_types::orders::PoolOrder;
use reth_primitives::revm_primitives::{Env, TransactTo, TxEnv};
use reth_provider::StateProviderFactory;
use reth_revm::EvmBuilder;
use revm::{db::WrapDatabaseRef, interpreter::opcode, Database, Inspector};

use self::{
    angstrom_pools::AngstromPools, angstrom_tokens::AngstromTokens, approvals::Approvals,
    balances::Balances, nonces::Nonces
};
use crate::common::lru_db::RevmLRU;

pub struct UserAccountDetails {
    pub token_bals:      (Address, U256),
    pub token_approvals: (Address, U256),
    pub is_valid_nonce:  bool,
    pub is_valid_pool:   bool,
    pub is_bid:          bool,
    pub pool_id:         usize
}

pub struct Upkeepers {
    new_pairs: AngstromTokens,
    approvals: Approvals,
    balances:  Balances,
    pools:     AngstromPools,
    nonces:    Nonces
}

impl Upkeepers {
    pub fn new<DB>(db: DB) -> Self {
        todo!()
    }

    pub fn verify_order<O: PoolOrder, DB: Send + StateProviderFactory>(
        &self,
        order: O,
        db: Arc<RevmLRU<DB>>
    ) -> (UserAccountDetails, O) {
        let is_valid_nonce = self
            .nonces
            .is_valid_nonce(order.from(), order.nonce(), db.clone());

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

    pub fn verify_composable_order<O: PoolOrder, DB: Send + StateProviderFactory>(
        &self,
        order: O,
        db: Arc<RevmLRU<DB>>,
        overrides: &HashMap<Address, HashMap<U256, U256>>
    ) -> (UserAccountDetails, O) {
        let is_valid_nonce = self
            .nonces
            .is_valid_nonce(order.from(), order.nonce(), db.clone());

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

    // update
    pub fn on_new_block(&mut self) {
        todo!()
    }
}

pub fn find_storage_slot<DB>(
    call_data: Bytes,
    wanted_address: Address,
    db: RevmLRU<DB>
) -> anyhow::Result<U256>
where
    DB: StateProviderFactory + Send + Sync + Clone + 'static
{
    let prob_address = Address::new([0, 0, 0, 0, 0, 0, 0, 0, 0, 11, 0, 0, 0, 0, 0, 64, 0, 0, 0, 0]);

    let mut tx_env = TxEnv::default();
    tx_env.transact_to = TransactTo::Call(wanted_address);
    tx_env.data = call_data.clone();
    tx_env.caller = prob_address.clone();

    let mut evm = EvmBuilder::default()
        .with_db(db.clone())
        .with_tx_env(tx_env.clone())
        .build();

    let res = U256::from_be_slice(&evm.transact().unwrap().result.output().unwrap().0);

    let one = U256::from(1);
    let prob_value = if res == U256::MAX { res - one } else { res + one };

    for i in 0..100 {
        let mut user_addr_encoded = prob_address.to_vec();
        user_addr_encoded.extend(U256::from(i).to_be_bytes::<32>().to_vec());

        let user_slot = U256::from_be_bytes(*keccak256(user_addr_encoded));
        let mut slot = HashMap::new();
        slot.insert(user_slot, prob_value.clone());
        let mut overrides = HashMap::new();
        overrides.insert(wanted_address.clone(), slot);
        let mut db = db.clone();
        db.set_state_overrides(overrides);

        let mut tx_env = TxEnv::default();
        tx_env.transact_to = TransactTo::Call(wanted_address);
        tx_env.data = call_data.clone();
        tx_env.caller = prob_address.clone();

        let mut evm = EvmBuilder::default()
            .with_db(db)
            .with_tx_env(tx_env.clone())
            .build();

        let res = U256::from_be_slice(&evm.transact().unwrap().result.output().unwrap().0);

        if res == prob_value {
            return Ok(U256::from(i))
        }
    }

    anyhow::bail!("should never be reached");
}
