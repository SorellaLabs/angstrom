use std::{
    pin::Pin,
    sync::{atomic::AtomicU64, Arc}
};

use alloy::primitives::{Address, BlockNumber, B256};
use angstrom_metrics::validation::ValidationMetrics;
use futures::Future;
use tokio::runtime::Handle;
use uniswap_v4::uniswap::pool_manager::SyncedUniswapPools;

use super::{
    sim::SimValidation,
    state::{
        account::user::UserAddress, db_state_utils::StateFetchUtils, pools::PoolsTracker,
        StateValidation
    },
    OrderValidationRequest
};
use crate::{
    common::{key_split_threadpool::KeySplitThreadpool, TokenPriceGenerator},
    order::{state::account::UserAccountProcessor, OrderValidation}
};

pub struct OrderValidator<DB, Pools, Fetch> {
    sim:          SimValidation<DB>,
    state:        StateValidation<Pools, Fetch>,
    block_number: Arc<AtomicU64>
}

impl<DB, Pools, Fetch> OrderValidator<DB, Pools, Fetch>
where
    DB: Unpin + Clone + 'static + revm::DatabaseRef + reth_provider::BlockNumReader + Sync + Send,
    <DB as revm::DatabaseRef>::Error: Send + Sync,
    Pools: PoolsTracker + Sync + 'static,
    Fetch: StateFetchUtils + Sync + 'static
{
    pub async fn new(
        sim: SimValidation<DB>,
        block_number: Arc<AtomicU64>,
        pools: Pools,
        fetch: Fetch,
        uniswap_pools: SyncedUniswapPools
    ) -> Self {
        let state = StateValidation::new(UserAccountProcessor::new(fetch), pools, uniswap_pools);

        Self { state, sim, block_number }
    }

    pub fn on_new_block(
        &mut self,
        block_number: BlockNumber,
        completed_orders: Vec<B256>,
        address_changes: Vec<Address>
    ) {
        self.block_number
            .store(block_number, std::sync::atomic::Ordering::SeqCst);
        self.state.new_block(completed_orders, address_changes);
    }

    /// only checks state
    pub fn validate_order(
        &mut self,
        order: OrderValidationRequest,
        token_conversion: TokenPriceGenerator,
        thread_pool: &mut KeySplitThreadpool<
            UserAddress,
            Pin<Box<dyn Future<Output = ()> + Send>>,
            Handle
        >,
        metrics: ValidationMetrics
    ) {
        let block_number = self.block_number.load(std::sync::atomic::Ordering::SeqCst);
        let order_validation: OrderValidation = order.into();
        let user = order_validation.user();
        let cloned_state = self.state.clone();
        let cloned_sim = self.sim.clone();

        thread_pool.add_new_task(
            user,
            Box::pin(async move {
                match order_validation {
                    OrderValidation::Limit(tx, order, _) => {
                        metrics.new_order(false, || {
                            let mut results = cloned_state.handle_regular_order(
                                order,
                                block_number,
                                metrics.clone()
                            );
                            results.add_gas_cost_or_invalidate(
                                &cloned_sim,
                                &token_conversion,
                                true
                            );

                            let _ = tx.send(results);
                        });
                    }
                    OrderValidation::Searcher(tx, order, _) => {
                        metrics.new_order(true, || {
                            let mut results = cloned_state.handle_regular_order(
                                order,
                                block_number,
                                metrics.clone()
                            );
                            results.add_gas_cost_or_invalidate(
                                &cloned_sim,
                                &token_conversion,
                                false
                            );

                            let _ = tx.send(results);
                        });
                    }
                    _ => unreachable!()
                }
            })
        );
    }
}
