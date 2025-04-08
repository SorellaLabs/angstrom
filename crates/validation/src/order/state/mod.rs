use std::sync::Arc;

use account::UserAccountProcessor;
use alloy::primitives::{Address, B256, U256};
use angstrom_metrics::validation::ValidationMetrics;
use angstrom_types::{
    primitive::OrderValidationError,
    sol_bindings::{ext::RawPoolOrder, grouped_orders::AllOrders, rpc_orders::TopOfBlockOrder}
};
use db_state_utils::StateFetchUtils;
use order_validators::{ORDER_VALIDATORS, OrderValidation, OrderValidationState};
use parking_lot::RwLock;
use pools::PoolsTracker;
use uniswap_v4::uniswap::pool_manager::SyncedUniswapPools;

use super::OrderValidationResults;

pub mod account;
pub mod config;
pub mod db_state_utils;
pub mod order_validators;
pub mod pools;

/// State validation is all validation that requires reading from the Ethereum
/// database, these operations are:
/// 1) validating order nonce,
/// 2) checking token balances
/// 3) checking token approvals
/// 4) deals with possible pending state
pub struct StateValidation<Pools, Fetch> {
    /// tracks everything user related.
    pub(crate) user_account_tracker: Arc<UserAccountProcessor<Fetch>>,
    /// tracks all info about the current angstrom pool state.
    pool_tacker:                     Arc<RwLock<Pools>>,
    /// keeps up-to-date with the on-chain pool
    uniswap_pools:                   SyncedUniswapPools
}

impl<Pools, Fetch> Clone for StateValidation<Pools, Fetch> {
    fn clone(&self) -> Self {
        Self {
            user_account_tracker: Arc::clone(&self.user_account_tracker),
            pool_tacker:          Arc::clone(&self.pool_tacker),
            uniswap_pools:        self.uniswap_pools.clone()
        }
    }
}

impl<Pools: PoolsTracker, Fetch: StateFetchUtils> StateValidation<Pools, Fetch> {
    pub fn new(
        user_account_tracker: UserAccountProcessor<Fetch>,
        pools: Pools,
        uniswap_pools: SyncedUniswapPools
    ) -> Self {
        Self {
            pool_tacker: Arc::new(RwLock::new(pools)),
            user_account_tracker: Arc::new(user_account_tracker),
            uniswap_pools
        }
    }

    pub fn new_block(&self, completed_orders: Vec<B256>, address_changes: Vec<Address>) {
        self.user_account_tracker
            .prepare_for_new_block(address_changes, completed_orders)
    }

    pub fn cancel_order(&self, user: Address, hash: B256) {
        self.user_account_tracker.cancel_order(user, hash);
    }

    pub fn validate<O: RawPoolOrder>(&self, order: &O) -> Result<(), OrderValidationError> {
        let mut state = OrderValidationState::new(order);

        for validator in ORDER_VALIDATORS {
            validator.validate_order(&mut state)?;
        }

        Ok(())
    }

    pub fn handle_regular_order<O: RawPoolOrder + Into<AllOrders>>(
        &self,
        order: O,
        block: u64,
        metrics: ValidationMetrics
    ) -> OrderValidationResults {
        metrics.applying_state_transitions(|| {
            let order_hash = order.order_hash();
            if !order.is_valid_signature() {
                tracing::debug!("order had invalid hash");
                return OrderValidationResults::Invalid {
                    hash:  order_hash,
                    error: OrderValidationError::InvalidSignature
                };
            }

            if let Err(e) = self.validate(&order) {
                tracing::warn!(?order, "invalid order");
                return OrderValidationResults::Invalid { hash: order_hash, error: e };
            };

            let Some(pool_info) = self.pool_tacker.read().fetch_pool_info_for_order(&order) else {
                tracing::debug!("order requested a invalid pool");
                return OrderValidationResults::Invalid {
                    hash:  order_hash,
                    error: OrderValidationError::InvalidPool
                };
            };

            self.user_account_tracker
                .verify_order::<O>(order, pool_info, block)
                .map(|o: _| {
                    OrderValidationResults::Valid(
                        o.try_map_inner(|inner| Ok(inner.into())).unwrap()
                    )
                })
                .unwrap_or_else(|e| {
                    tracing::debug!(%e,"user account tracker failed to validate order");
                    OrderValidationResults::Invalid {
                        hash:  order_hash,
                        error: OrderValidationError::StateError(e)
                    }
                })
        })
    }

    pub async fn handle_tob_order(
        &self,
        order: TopOfBlockOrder,
        block: u64,
        metrics: ValidationMetrics
    ) -> OrderValidationResults {
        let order_hash = order.order_hash();
        let mut results = self.handle_regular_order(order, block, metrics);
        let mut invalidate = false;

        if let OrderValidationResults::Valid(ref mut tob_order) = results {
            let tob_orders = tob_order
                .clone()
                .try_map_inner(|inner| {
                    let AllOrders::TOB(order) = inner else { eyre::bail!("unreachable") };
                    Ok(order)
                })
                .expect("should be unreachable");
            let pool_address = tob_orders.pool_id;
            if let Ok(total_reward) = self
                .uniswap_pools
                .calculate_rewards(pool_address, &tob_orders)
                .await
            {
                tob_order.tob_reward = U256::from(total_reward);
            } else {
                invalidate = true;
            }
        }

        if invalidate {
            return OrderValidationResults::Invalid {
                hash:  order_hash,
                error: OrderValidationError::InvalidToBSwap
            };
        }

        results
    }
}
#[cfg(test)]
mod test {

    use std::sync::Arc;

    use alloy::primitives::U256;
    use dashmap::DashMap;
    use testing_tools::type_generator::orders::UserOrderBuilder;
    use uniswap_v4::uniswap::pool_manager::SyncedUniswapPools;

    use super::{
        StateValidation, account::UserAccountProcessor, db_state_utils::test_fetching::MockFetch,
        pools::pool_tracker_mock::MockPoolTracker
    };
    #[test]
    fn test_order_of_validators() {
        let mock_pools = MockPoolTracker::default();
        let mock_fetch = MockFetch::default();
        let (tx, _) = tokio::sync::mpsc::channel(10);
        let pools = SyncedUniswapPools::new(Arc::new(DashMap::new()), tx);
        let validator =
            StateValidation::new(UserAccountProcessor::new(mock_fetch), mock_pools, pools);

        let order = UserOrderBuilder::new()
            .standing()
            .exact()
            .amount(1)
            .min_price(U256::ZERO.into())
            .ask()
            .build();
        validator.validate(&order).unwrap_err();
    }
}
