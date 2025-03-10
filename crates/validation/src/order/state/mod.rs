use std::sync::Arc;

use account::UserAccountProcessor;
use alloy::primitives::{Address, B256, U256};
use angstrom_metrics::validation::ValidationMetrics;
use angstrom_types::{
    primitive::OrderValidationError,
    sol_bindings::{
        Ray, ext::RawPoolOrder, grouped_orders::AllOrders, rpc_orders::TopOfBlockOrder
    }
};
use db_state_utils::StateFetchUtils;
use parking_lot::RwLock;
use pools::PoolsTracker;
use uniswap_v4::uniswap::pool_manager::SyncedUniswapPools;

use super::OrderValidationResults;

pub mod account;
pub mod config;
pub mod db_state_utils;
pub mod pools;

/// State validation is all validation that requires reading from the Ethereum
/// database, these operations are:
/// 1) validating order nonce,
/// 2) checking token balances
/// 3) checking token approvals
/// 4) deals with possible pending state
pub struct StateValidation<Pools, Fetch> {
    /// tracks everything user related.
    user_account_tracker: Arc<UserAccountProcessor<Fetch>>,
    /// tracks all info about the current angstrom pool state.
    pool_tacker:          Arc<RwLock<Pools>>,
    /// keeps up-to-date with the on-chain pool
    uniswap_pools:        SyncedUniswapPools
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

    fn fetch_min_qty_in_t0<O: RawPoolOrder>(&self, order: &O) -> u128 {
        if !order.is_bid() {
            if order.exact_in() {
                order.min_amount()
            } else {
                Ray::from(order.limit_price()).inverse_quantity(order.min_amount(), true)
            }
        } else if order.exact_in() {
            Ray::from(order.limit_price())
                .mul_quantity(U256::from(order.min_amount()))
                .to::<u128>()
        } else {
            order.min_amount()
        }
    }

    pub fn correctly_built<O: RawPoolOrder>(&self, order: &O) -> bool {
        // ensure max gas is less than the min amount they can be filled
        let min_qty = self.fetch_min_qty_in_t0(order);
        min_qty >= order.max_gas_token_0()
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

            if !self.correctly_built(&order) {
                tracing::info!(?order, "invalidly built order");
                return OrderValidationResults::Invalid {
                    hash:  order_hash,
                    error: OrderValidationError::InvalidPartialOrder
                };
            }

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
            if let Ok(rewards) = self
                .uniswap_pools
                .calculate_rewards(pool_address, &tob_orders)
                .await
            {
                tob_order.tob_reward = U256::from(rewards.total_reward);
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
