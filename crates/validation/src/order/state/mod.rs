use std::{collections::HashMap, sync::Arc, task::Poll};

use account::UserAccountTracker;
use alloy_primitives::{Address, B256, U256};
use angstrom_types::sol_bindings::grouped_orders::{AllOrders, RawPoolOrder};
use fetch_utils::index_to_address::AssetIndexToAddressWrapper;
use futures::{Stream, StreamExt};
use futures_util::stream::FuturesUnordered;
use parking_lot::RwLock;
use revm::db::{AccountStatus, BundleState};
use tokio::{
    sync::oneshot::Sender,
    task::{yield_now, JoinHandle}
};

use self::{
    fetch_utils::{Upkeepers, UserAccountDetails},
    orders::UserOrders
};
use super::OrderValidation;
use crate::{
    common::{
        executor::ThreadPool,
        lru_db::{BlockStateProviderFactory, RevmLRU}
    },
    order::state::config::ValidationConfig
};

pub mod account;
pub mod config;
pub mod fetch_utils;
pub mod orders;

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
    user_account_tracker: Arc<RwLock<UserAccountTracker>>
}

impl<DB> StateValidation<DB>
where
    DB: BlockStateProviderFactory + Unpin + 'static
{
    pub fn new(db: Arc<RevmLRU<DB>>, config: ValidationConfig) -> Self {
        Self { db, upkeepers: Arc::new(RwLock::new(Upkeepers::new(config))) }
    }

    pub fn wrap_order<O: RawPoolOrder>(&self, order: O) -> Option<AssetIndexToAddressWrapper<O>> {
        self..read().asset_to_address.wrap(order)
    }

    pub fn validate_regular_order(
        &self,
        order: OrderValidation
    ) -> Option<(OrderValidation, UserAccountDetails)> {
        let db = self.db.clone();
        let keeper = self.upkeepers.clone();

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

    pub fn validate_state_prehook(
        &self,
        order: OrderValidation,
        prehook_state_deltas: &HookOverrides
    ) -> Option<(OrderValidation, UserAccountDetails)> {
        let db = self.db.clone();
        let keeper = self.upkeepers.clone();

        match order {
            OrderValidation::LimitComposable(tx, o, origin) => {
                let (details, order) =
                    keeper
                        .read()
                        .verify_composable_order(o, db, prehook_state_deltas)?;
                Some((OrderValidation::LimitComposable(tx, order, origin), details))
            }
            _ => unreachable!()
        }
    }

    pub fn validate_state_posthook(
        &self,
        order: OrderValidation,
        prehook_state_deltas: &HookOverrides
    ) -> Option<(OrderValidation, UserAccountDetails)> {
        let db = self.db.clone();
        let keeper = self.upkeepers.clone();

        match order {
            OrderValidation::LimitComposable(tx, o, origin) => {
                let (details, order) =
                    keeper
                        .read()
                        .verify_composable_order(o, db, prehook_state_deltas)?;
                Some((OrderValidation::LimitComposable(tx, order, origin), details))
            }
            _ => unreachable!()
        }
    }
}
