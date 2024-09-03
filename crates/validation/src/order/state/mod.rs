use std::{collections::HashMap, sync::Arc, task::Poll};

use account::UserAccountProcessor;
use alloy_primitives::{Address, B256, U256};
use angstrom_types::sol_bindings::grouped_orders::{AllOrders, RawPoolOrder};
use futures::{Stream, StreamExt};
use futures_util::stream::FuturesUnordered;
use parking_lot::RwLock;
use pools::AngstromPoolsTracker;
use revm::db::{AccountStatus, BundleState};
use tokio::{
    sync::oneshot::Sender,
    task::{yield_now, JoinHandle}
};

use self::db_state_utils::UserAccountDetails;
use super::{OrderValidation, OrderValidationResults};
use crate::{
    common::{
        executor::ThreadPool,
        lru_db::{BlockStateProviderFactory, RevmLRU}
    },
    order::state::{config::ValidationConfig, pools::index_to_address::AssetIndexToAddressWrapper}
};

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
pub struct StateValidation<DB> {
    db:                   Arc<RevmLRU<DB>>,
    /// tracks everything user related.
    user_account_tracker: Arc<UserAccountProcessor<DB>>,
    /// tracks all info about the current angstrom pool state.
    pool_tacker:          Arc<AngstromPoolsTracker>
}

impl<DB> StateValidation<DB>
where
    DB: BlockStateProviderFactory + Unpin + 'static
{
    pub fn new(db: Arc<RevmLRU<DB>>, config: ValidationConfig) -> Self {
        todo!()
    }

    pub fn wrap_order<O: RawPoolOrder>(&self, order: O) -> Option<AssetIndexToAddressWrapper<O>> {
        self.pool_tacker.asset_index_to_address.wrap(order)
    }

    fn handle_regular_order<O: RawPoolOrder + Into<AllOrders>>(
        &self,
        order: O,
        block: u64,
        is_limit: bool
    ) -> OrderValidationResults {
        let order_hash = order.hash();
        let Some((pool_info, wrapped_order)) = self.pool_tacker.fetch_pool_info_for_order(order)
        else {
            return OrderValidationResults::Invalid(order_hash)
        };

        self.user_account_tracker
            .verify_order(wrapped_order, pool_info, block, is_limit)
            .map(|o| {
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
}
