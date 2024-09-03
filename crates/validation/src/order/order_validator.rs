use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    marker::PhantomData,
    pin::Pin,
    sync::{atomic::AtomicU64, Arc},
    task::{Context, Poll}
};

use alloy_primitives::{Address, U256};
use angstrom_utils::sync_pipeline::{
    PipelineAction, PipelineBuilder, PipelineFut, PipelineOperation, PipelineWithIntermediary
};
use futures::{stream::FuturesUnordered, Future, StreamExt};
use futures_util::{future, FutureExt, Stream};
use tokio::{runtime::Handle, task::JoinHandle};

use super::{
    sim::SimValidation,
    state::{config::ValidationConfig, StateValidation},
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
    block_number: Arc<AtomicU64>
}

impl<DB> OrderValidator<DB>
where
    DB: BlockStateProviderFactory + Unpin + Clone + 'static
{
    pub fn new(
        db: Arc<RevmLRU<DB>>,
        config: ValidationConfig,
        block_number: Arc<AtomicU64>
    ) -> Self {
        let state = StateValidation::new(db.clone(), config);
        let sim = SimValidation::new(db);

        let new_state = state.clone();
        let new_sim = sim.clone();

        // let pipeline = PipelineBuilder::new()
        //     .add_step(0, ValidationOperation::pre_regular_verification)
        //     .add_step(1, ValidationOperation::post_regular_verification)
        //     // .add_step(2, ValidationOperation::pre_hook_sim)
        //     .build(tokio::runtime::Handle::current());

        Self { state, sim, block_number }
    }

    pub fn update_block_number(&mut self, number: u64) {
        self.block_number
            .store(number, std::sync::atomic::Ordering::SeqCst);
    }

    /// only checks state
    pub fn validate_order(&mut self, order: OrderValidationRequest) {
        let block_number = self.block_number.load(std::sync::atomic::Ordering::SeqCst);
        let order_validation: OrderValidation = order.into();
        // match order_validation {
        //     order @ OrderValidation::Limit(..) => {
        //         self.pipeline
        //             .add(ValidationOperation::PreRegularVerification(order,
        // block_number));     }
        //     order @ OrderValidation::Searcher(..) => self
        //         .pipeline
        //         .add(ValidationOperation::PreRegularVerification(order,
        // block_number)),
        //
        //     order @ OrderValidation::LimitComposable(..) => self
        //         .pipeline
        //         .add(ValidationOperation::PreHookSim(order, block_number))
        // }
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
        let state = self.state.clone();
        let sim = self.sim.clone();
        let mut ctx = ProcessingCtx::new(
            &mut self.orders as *mut UserOrders,
            sim,
            state,
            self.block_number.clone()
        );

        while let Poll::Ready(Some(_)) = self.pipeline.poll(cx, &mut ctx) {}

        Poll::Pending
    }
}
