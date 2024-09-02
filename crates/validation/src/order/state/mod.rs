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
use super::OrderValidation;
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
    user_account_tracker: Arc<RwLock<UserAccountProcessor<DB>>>,
    /// tracks all info about the current angstrom pool state.
    pool_tacker:          Arc<RwLock<AngstromPoolsTracker>>
}

impl<DB> StateValidation<DB>
where
    DB: BlockStateProviderFactory + Unpin + 'static
{
    pub fn new(db: Arc<RevmLRU<DB>>, config: ValidationConfig) -> Self {
        todo!()
    }

    pub fn wrap_order<O: RawPoolOrder>(&self, order: O) -> Option<AssetIndexToAddressWrapper<O>> {
        self.pool_tacker.read().asset_index_to_address.wrap(order)
    }

    pub fn validate_regular_order(
        &self,
        order: OrderValidation
    ) -> Option<(OrderValidation, UserAccountDetails)> {
        let db = self.db.clone();

        match order {
            OrderValidation::Limit(tx, o, origin) => {
                let (details, order) = keeper.read().verify_order(o, db)?;
                Some((OrderValidation::Limit(tx, order, origin), details))
            }
            OrderValidation::Searcher(tx, o, origin) => {
                let (details, order) = keeper.read().verify_order(o, db)?;
                Some((OrderValidation::Searcher(tx, order, origin), details))
            }
            _ => unreachable!()
        }
    }
}
