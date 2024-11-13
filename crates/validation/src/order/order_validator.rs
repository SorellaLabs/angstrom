use std::{
    pin::Pin,
    sync::{atomic::AtomicU64, Arc},
    task::Poll
};

use alloy::primitives::{Address, BlockNumber, B256};
use angstrom_types::{pair_with_price::PairsWithPrice, primitive::NewInitializedPool};
use angstrom_utils::key_split_threadpool::KeySplitThreadpool;
use futures::{Future, Stream, StreamExt};
use matching_engine::cfmm::uniswap::pool_manager::SyncedUniswapPools;
use tokio::runtime::Handle;

use super::{
    sim::SimValidation,
    state::{
        account::user::UserAddress, db_state_utils::StateFetchUtils, pools::PoolsTracker,
        token_pricing::TokenPriceGenerator, StateValidation
    },
    OrderValidationRequest
};
use crate::order::{state::account::UserAccountProcessor, OrderValidation};

pub struct OrderValidator<DB, Pools, Fetch> {
    sim:              SimValidation<DB>,
    state:            StateValidation<Pools, Fetch>,
    token_conversion: TokenPriceGenerator,
    token_updates:    Pin<Box<dyn Stream<Item = Vec<PairsWithPrice>> + 'static>>,
    thread_pool: KeySplitThreadpool<UserAddress, Pin<Box<dyn Future<Output = ()> + Send>>, Handle>,
    block_number:     Arc<AtomicU64>
}

impl<DB, Pools, Fetch> OrderValidator<DB, Pools, Fetch>
where
    DB: Unpin + Clone + 'static + revm::DatabaseRef + reth_provider::BlockNumReader + Sync + Send,
    <DB as revm::DatabaseRef>::Error: Send + Sync,
    Pools: PoolsTracker + Sync + 'static,
    Fetch: StateFetchUtils + Sync + 'static
{
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        sim: SimValidation<DB>,
        block_number: Arc<AtomicU64>,
        pools: Pools,
        fetch: Fetch,
        uniswap_pools: SyncedUniswapPools,
        thread_pool: KeySplitThreadpool<
            UserAddress,
            Pin<Box<dyn Future<Output = ()> + Send>>,
            Handle
        >,
        token_conversion: TokenPriceGenerator,
        token_updates: Pin<Box<dyn Stream<Item = Vec<PairsWithPrice>> + 'static>>
    ) -> Self {
        let state = StateValidation::new(UserAccountProcessor::new(fetch), pools, uniswap_pools);

        Self { state, sim, block_number, thread_pool, token_conversion, token_updates }
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
    pub fn validate_order(&mut self, order: OrderValidationRequest) {
        let block_number = self.block_number.load(std::sync::atomic::Ordering::SeqCst);
        let order_validation: OrderValidation = order.into();
        let user = order_validation.user();
        let cloned_state = self.state.clone();
        let cloned_sim = self.sim.clone();
        let cloned_price = self.token_conversion.clone();

        self.thread_pool.add_new_task(
            user,
            Box::pin(async move {
                match order_validation {
                    OrderValidation::Limit(tx, order, _) => {
                        let mut results = cloned_state.handle_regular_order(order, block_number);
                        results.add_gas_cost_or_invalidate(&cloned_sim, &cloned_price, true);

                        let _ = tx.send(results);
                    }
                    OrderValidation::Searcher(tx, order, _) => {
                        let mut results = cloned_state.handle_regular_order(order, block_number);
                        results.add_gas_cost_or_invalidate(&cloned_sim, &cloned_price, false);

                        let _ = tx.send(results);
                    }
                    _ => unreachable!()
                }
            })
        );
    }
}

impl<DB, Pools, Fetch> Future for OrderValidator<DB, Pools, Fetch>
where
    DB: Clone + Unpin + 'static + revm::DatabaseRef + Send + Sync,
    <DB as revm::DatabaseRef>::Error: Send + Sync,
    Pools: PoolsTracker + Sync + Unpin + 'static,
    Fetch: StateFetchUtils + Sync + Unpin + 'static
{
    type Output = ();

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>
    ) -> std::task::Poll<Self::Output> {
        self.thread_pool.try_register_waker(|| cx.waker().clone());
        while let Poll::Ready(Some(updates)) = self.token_updates.poll_next_unpin(cx) {
            self.token_conversion.apply_update(updates);
        }

        while let Poll::Ready(Some(_)) = self.thread_pool.poll_next_unpin(cx) {}

        Poll::Pending
    }
}
