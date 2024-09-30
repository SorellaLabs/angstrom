use std::{collections::HashMap, sync::Arc};

use account::UserAccountProcessor;
use alloy::primitives::{Address, B256, U256};
use angstrom_types::{
    primitive::NewInitializedPool,
    sol_bindings::{ext::RawPoolOrder, grouped_orders::AllOrders}
};
use db_state_utils::StateFetchUtils;
use futures::{Stream, StreamExt};
use parking_lot::RwLock;
use pools::PoolsTracker;

use super::{OrderValidation, OrderValidationResults};
use crate::common::lru_db::{BlockStateProviderFactory, RevmLRU};

pub mod account;
pub mod config;
pub mod db_state_utils;
pub mod pools;

type HookOverrides = HashMap<Address, HashMap<U256, U256>>;

/// State validation is all validation that requires reading from the Ethereum
/// database, these operations are:
/// 1) validating order nonce,
/// 2) checking token balances
/// 3) checking token approvals
/// 4) deals with possible pending state
#[allow(dead_code)]
#[derive(Clone)]
pub struct StateValidation<Pools, Fetch> {
    /// tracks everything user related.
    user_account_tracker: Arc<UserAccountProcessor<Fetch>>,
    /// tracks all info about the current angstrom pool state.
    pool_tacker:          Arc<RwLock<Pools>>
}

impl<Pools: PoolsTracker, Fetch: StateFetchUtils> StateValidation<Pools, Fetch> {
    pub fn new(user_account_tracker: UserAccountProcessor<Fetch>, pools: Pools) -> Self {
        Self {
            pool_tacker:          Arc::new(RwLock::new(pools)),
            user_account_tracker: Arc::new(user_account_tracker)
        }
    }

    pub fn new_block(
        &self,
        number: u64,
        completed_orders: Vec<B256>,
        address_changes: Vec<Address>
    ) {
        self.user_account_tracker
            .prepare_for_new_block(address_changes, completed_orders)
    }

    fn handle_regular_order<O: RawPoolOrder + Into<AllOrders>>(
        &self,
        order: O,
        block: u64,
        is_limit: bool
    ) -> OrderValidationResults {
        let order_hash = order.order_hash();
        if !order.is_valid_signature() {
            return OrderValidationResults::Invalid(order_hash)
        }

        let Some(pool_info) = self
            .pool_tacker
            .read_arc()
            .fetch_pool_info_for_order(&order)
        else {
            return OrderValidationResults::Invalid(order_hash)
        };

        self.user_account_tracker
            .verify_order::<O>(order, pool_info, block, is_limit)
            .map(|o: _| {
                OrderValidationResults::Valid(o.try_map_inner(|inner| Ok(inner.into())).unwrap())
            })
            .unwrap_or_else(|_| OrderValidationResults::Invalid(order_hash))
    }

    pub fn validate_state_of_regular_order(&self, order: OrderValidation, block: u64) {
        match order {
            OrderValidation::Limit(tx, order, origin) => {
                let results = self.handle_regular_order(order, block, true);
                let _ = tx.send(results);
            }
            OrderValidation::Searcher(tx, order, origin) => {
                let results = self.handle_regular_order(order, block, false);
                let _ = tx.send(results);
            }
            _ => unreachable!()
        }
    }

    pub fn index_new_pool(&mut self, pool: NewInitializedPool) {
        self.pool_tacker.write_arc().index_new_pool(pool);
    }
}
