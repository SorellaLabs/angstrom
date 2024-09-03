use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    marker::PhantomData,
    pin::Pin,
    sync::{atomic::AtomicU64, Arc},
    task::{Context, Poll}
};

use alloy_primitives::{Address, U256};
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
    state::{account::user::UserAddress, config::ValidationConfig, StateValidation},
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

pub struct OrderValidator<DB> {
    sim:          SimValidation<DB>,
    state:        StateValidation<DB>,
    threadpool:   KeySplitThreadpool<UserAddress, Pin<Box<dyn Future<Output = ()> + Send>>, Handle>,
    block_number: Arc<AtomicU64>
}

impl<DB> OrderValidator<DB>
where
    DB: BlockStateProviderFactory + Unpin + Clone + 'static
{
    pub fn new(
        db: Arc<RevmLRU<DB>>,
        config: ValidationConfig,
        block_number: Arc<AtomicU64>,
        handle: Handle
    ) -> Self {
        let threadpool = KeySplitThreadpool::new(handle, config.max_validation_per_user);
        let state = StateValidation::new(db.clone(), config);
        let sim = SimValidation::new(db);

        let new_state = state.clone();
        let new_sim = sim.clone();

        Self { state, sim, block_number, threadpool }
    }

    pub fn update_block_number(&mut self, number: u64) {
        self.block_number
            .store(number, std::sync::atomic::Ordering::SeqCst);
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

impl<DB> Future for OrderValidator<DB>
where
    DB: BlockStateProviderFactory + Clone + Unpin + 'static
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
