use std::sync::Arc;

use account::UserAccountProcessor;
use alloy::primitives::{Address, B256};
use angstrom_metrics::validation::ValidationMetrics;
use angstrom_types::{
    primitive::{OrderValidationError, UserAccountVerificationError, UserOrderPoolInfo},
    sol_bindings::{
        Ray,
        ext::RawPoolOrder,
        grouped_orders::{AllOrders, OrderWithStorageData},
        rpc_orders::TopOfBlockOrder
    }
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

    pub async fn handle_orders<O: RawPoolOrder + Into<AllOrders>>(
        &self,
        order: O,
        block: u64,
        metrics: ValidationMetrics,
        is_revalidating: bool,
        tob_rewards: impl AsyncFnOnce(
            &mut O,
            &UserOrderPoolInfo
        ) -> Result<(u128, u128), UserAccountVerificationError>
    ) -> OrderValidationResults {
        metrics
            .applying_state_transitions(async || {
                let order_hash = order.order_hash();
                if !order.is_valid_signature() {
                    tracing::debug!("order had invalid hash");
                    return OrderValidationResults::Invalid {
                        hash:  order_hash,
                        error: OrderValidationError::InvalidSignature
                    };
                }

                if let Err(e) = self.validate(&order) {
                    tracing::warn!(?e, ?order, "invalid order");
                    return OrderValidationResults::Invalid { hash: order_hash, error: e };
                };

                let Some(pool_info) = self.pool_tacker.read().fetch_pool_info_for_order(&order)
                else {
                    tracing::debug!("order requested a invalid pool");
                    return OrderValidationResults::Invalid {
                        hash:  order_hash,
                        error: OrderValidationError::InvalidPool
                    };
                };

                self.user_account_tracker
                    .verify_order::<O>(order, pool_info, block, is_revalidating, tob_rewards)
                    .await
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
            .await
    }

    pub async fn handle_tob_order(
        &self,
        order: TopOfBlockOrder,
        block: u64,
        conversion_rate: Ray,
        metrics: ValidationMetrics
    ) -> OrderValidationResults {
        self.handle_orders(order, block, metrics.clone(), false, async |order, pool_info| {
            let pool_address = pool_info.pool_id;
            // lifetimes :(
            let with_storage = OrderWithStorageData::with_default(order.clone());
            let total_reward = metrics
                .v4_sim(async || {
                    self.uniswap_pools
                        .calculate_rewards(pool_address, &with_storage)
                        .await
                        .map_err(|_| UserAccountVerificationError::InvalidToBSwap)
                })
                .await?;

            // given the price is always t1 / t0,
            let rewards_in_token_in = if order.is_bid() {
                conversion_rate.quantity(total_reward, false)
            } else {
                total_reward
            };

            Ok((total_reward, rewards_in_token_in))
        })
        .await
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
