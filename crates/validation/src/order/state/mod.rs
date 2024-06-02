use std::{collections::HashMap, sync::Arc, task::Poll};

use alloy_primitives::{Address, B256, U256};
use angstrom_types::orders::{OrderValidationOutcome, PoolOrder};
use futures::{Stream, StreamExt};
use futures_util::stream::FuturesUnordered;
use parking_lot::RwLock;
use revm::db::{AccountStatus, BundleState};
use tokio::{
    sync::oneshot::Sender,
    task::{yield_now, JoinHandle}
};

use self::{
    orders::UserOrders,
    upkeepers::{Upkeepers, UserAccountDetails}
};
use super::OrderValidationRequest;
use crate::{
    common::{
        executor::ThreadPool,
        lru_db::{BlockStateProviderFactory, RevmLRU}
    },
    order::state::config::ValidationConfig
};

pub mod config;
pub mod orders;
pub mod upkeepers;

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
    db:        Arc<RevmLRU<DB>>,
    /// upkeeps all state specific checks.
    upkeepers: Arc<RwLock<Upkeepers>>
}

impl<DB> StateValidation<DB>
where
    DB: BlockStateProviderFactory + Unpin + 'static
{
    pub fn new(db: Arc<RevmLRU<DB>>, config: ValidationConfig) -> Self {
        Self { db, upkeepers: Arc::new(RwLock::new(Upkeepers::new(config))) }
    }

    pub fn validate_regular_order(
        &self,
        order: OrderValidationRequest
    ) -> (OrderValidationRequest, UserAccountDetails) {
        let db = self.db.clone();
        let keeper = self.upkeepers.clone();

        match order {
            OrderValidationRequest::ValidateLimit(tx, origin, o) => {
                let (details, order) = keeper.read().verify_order(o, db);
                (OrderValidationRequest::ValidateLimit(tx, origin, order), details)
            }
            OrderValidationRequest::ValidateSearcher(tx, origin, o) => {
                let (details, order) = keeper.read().verify_order(o, db);
                (OrderValidationRequest::ValidateSearcher(tx, origin, order), details)
            }
            _ => unreachable!()
        }
    }

    pub fn validate_state_prehook(
        &self,
        order: OrderValidationRequest,
        prehook_state_deltas: &HookOverrides
    ) -> (OrderValidationRequest, UserAccountDetails) {
        let db = self.db.clone();
        let keeper = self.upkeepers.clone();

        match order {
            OrderValidationRequest::ValidateComposableLimit(tx, origin, o) => {
                let (details, order) =
                    keeper
                        .read()
                        .verify_composable_order(o, db, prehook_state_deltas);
                (OrderValidationRequest::ValidateComposableLimit(tx, origin, order), details)
            }
            OrderValidationRequest::ValidateComposableSearcher(tx, origin, o) => {
                let (details, order) =
                    keeper
                        .read()
                        .verify_composable_order(o, db, prehook_state_deltas);
                (OrderValidationRequest::ValidateComposableSearcher(tx, origin, order), details)
            }
            _ => unreachable!()
        }
    }

    pub fn validate_state_posthook(
        &self,
        order: OrderValidationRequest,
        prehook_state_deltas: &HookOverrides
    ) -> (OrderValidationRequest, UserAccountDetails) {
        let db = self.db.clone();
        let keeper = self.upkeepers.clone();

        match order {
            OrderValidationRequest::ValidateComposableLimit(tx, origin, o) => {
                let (details, order) =
                    keeper
                        .read()
                        .verify_composable_order(o, db, prehook_state_deltas);
                (OrderValidationRequest::ValidateComposableLimit(tx, origin, order), details)
            }
            OrderValidationRequest::ValidateComposableSearcher(tx, origin, o) => {
                let (details, order) =
                    keeper
                        .read()
                        .verify_composable_order(o, db, prehook_state_deltas);
                (OrderValidationRequest::ValidateComposableSearcher(tx, origin, order), details)
            }
            _ => unreachable!()
        }
    }
}
