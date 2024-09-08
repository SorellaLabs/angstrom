use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    marker::PhantomData,
    pin::Pin,
    sync::{atomic::AtomicU64, Arc},
    task::{Context, Poll}
};

use alloy_primitives::{Address, B256, U256};
use angstrom_utils::{
    key_split_threadpool::KeySplitThreadpool,
    sync_pipeline::{
        PipelineAction, PipelineBuilder, PipelineFut, PipelineOperation, PipelineWithIntermediary
    }
};
use futures::{stream::FuturesUnordered, Future, StreamExt};
use futures_util::{future, FutureExt, Stream};
use tokio::{runtime::Handle, task::JoinHandle};

use super::{
    sim::SimValidation,
    state::{
        account::user::UserAddress, config::ValidationConfig, db_state_utils::StateFetchUtils,
        pools::PoolsTracker, StateValidation
    },
    OrderValidationRequest
};
use crate::{
    common::{
        executor::ThreadPool,
        lru_db::{BlockStateProviderFactory, RevmLRU}
    },
    order::{sim, OrderValidation},
    validator::ValidationRequest
};

pub struct OrderValidator<DB, Pools, Fetch> {
    sim:          SimValidation<DB>,
    state:        StateValidation<DB, Pools, Fetch>,
    threadpool:   KeySplitThreadpool<UserAddress, Pin<Box<dyn Future<Output = ()> + Send>>, Handle>,
    block_number: Arc<AtomicU64>
}

impl<DB, Pools, Fetch> OrderValidator<DB, Pools, Fetch>
where
    DB: BlockStateProviderFactory + Unpin + Clone + 'static,
    Pools: PoolsTracker + Sync + 'static,
    Fetch: StateFetchUtils + Sync + 'static
{
    pub fn new(
        db: Arc<RevmLRU<DB>>,
        block_number: Arc<AtomicU64>,
        max_validation_per_user: usize,
        pools: Pools,
        fetch: Fetch,
        handle: Handle
    ) -> Self {
        let threadpool = KeySplitThreadpool::new(handle, max_validation_per_user);
        let state = StateValidation::new(
            db.clone(),
            block_number.load(std::sync::atomic::Ordering::SeqCst),
            pools,
            fetch
        );
        let sim = SimValidation::new(db);

        let new_state = state.clone();
        let new_sim = sim.clone();

        Self { state, sim, block_number, threadpool }
    }

    pub fn on_new_block(
        &mut self,
        number: u64,
        completed_orders: Vec<B256>,
        address_changes: Vec<Address>
    ) {
        self.block_number
            .store(number, std::sync::atomic::Ordering::SeqCst);
        self.state
            .new_block(number, completed_orders, address_changes);
    }

    /// only checks state
    pub fn validate_order(&mut self, order: OrderValidationRequest) {
        let block_number = self.block_number.load(std::sync::atomic::Ordering::SeqCst);
        let order_validation: OrderValidation = order.into();
        let user = order_validation.user();
        let cloned_state = self.state.clone();

        self.threadpool.add_new_task(
            user,
            Box::pin(async move {
                cloned_state.validate_state_of_regular_order(order_validation, block_number)
            })
        );
    }
}

impl<DB, Pools, Fetch> Future for OrderValidator<DB, Pools, Fetch>
where
    DB: BlockStateProviderFactory + Clone + Unpin + 'static,
    Pools: PoolsTracker + Sync + Unpin + 'static,
    Fetch: StateFetchUtils + Sync + Unpin + 'static
{
    type Output = ();

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>
    ) -> std::task::Poll<Self::Output> {
        self.threadpool.try_register_waker(|| cx.waker().clone());

        while let Poll::Ready(Some(_)) = self.threadpool.poll_next_unpin(cx) {}

        Poll::Pending
    }
}
