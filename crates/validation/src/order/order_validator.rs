use std::{
    collections::VecDeque,
    marker::PhantomData,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll}
};

use alloy_primitives::{Address, U256};
use futures::{stream::FuturesUnordered, Future, StreamExt};
use futures_util::{future, FutureExt, Stream};
use guard_types::orders::{OrderValidationOutcome, PoolOrder, ValidatedOrder, ValidationResults};
use guard_utils::sync_pipeline::{
    FnPtr, PipelineAction, PipelineBuilder, PipelineFut, PipelineOperation,
    PipelineWithIntermediary
};
use reth_provider::StateProviderFactory;
use revm::primitives::HashMap;
use tokio::{runtime::Handle, task::JoinHandle};

use super::{
    sim::SimValidation,
    state::{orders::UserOrders, upkeepers::UserAccountDetails, StateValidation},
    OrderValidationRequest
};
use crate::{
    common::{executor::ThreadPool, lru_db::RevmLRU},
    order::sim,
    validator::ValidationRequest
};

#[allow(dead_code)]
pub struct OrderValidator<DB> {
    sim:    SimValidation<DB>,
    state:  StateValidation<DB>,
    orders: UserOrders,

    pipeline: PipelineWithIntermediary<Handle, ValidationOperation, UserOrders>
}

pub struct ProcessingCtx<'a, DB>
where
    DB: StateProviderFactory + Unpin + Clone + 'static
{
    pub user_orders: &'a mut UserOrders,
    pub sim:         SimValidation<DB>,
    pub state:       StateValidation<DB>
}

impl<DB> OrderValidator<DB>
where
    DB: StateProviderFactory + Unpin + Clone + 'static
{
    pub fn new(db: Arc<RevmLRU<DB>>) -> Self {
        let state = StateValidation::new(db.clone());
        let sim = SimValidation::new(db);

        let new_state = state.clone();
        let new_sim = sim.clone();

        let pipeline = PipelineBuilder::new()
            .add_step(0, FnPtr::new(pre_regular_verification))
            .add_step(1, FnPtr::new(post_regular_verification))
            .add_step(2, FnPtr::new(pre_hook_sim))
            .add_step(3, FnPtr::new(post_pre_hook_sim))
            .add_step(4, FnPtr::new(post_hook_sim))
            .build(tokio::runtime::Handle::current());

        Self { state, sim, pipeline, orders: UserOrders::new() }
    }

    /// only checks state
    pub fn validate_order(&mut self, order: OrderValidationRequest) {
        match order {
            order @ OrderValidationRequest::ValidateLimit(..) => {
                self.pipeline
                    .add(ValidationOperation::PreRegularVerification(order));
            }
            order @ OrderValidationRequest::ValidateSearcher(..) => self
                .pipeline
                .add(ValidationOperation::PreRegularVerification(order)),

            order @ OrderValidationRequest::ValidateComposableLimit(..) => {
                self.pipeline.add(ValidationOperation::PreHookSim(order))
            }

            order @ OrderValidationRequest::ValidateComposableSearcher(..) => {
                self.pipeline.add(ValidationOperation::PreHookSim(order))
            }
        }
    }
}

impl<DB> Future for OrderValidator<DB>
where
    DB: StateProviderFactory + Unpin + 'static
{
    type Output = ();

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>
    ) -> std::task::Poll<Self::Output> {
        let orders = &mut self.orders;
        while let Poll::Ready(Some(_)) = self.pipeline.poll(orders, cx) {}
        Poll::Pending
    }
}

pub enum ValidationOperation {
    PreRegularVerification(OrderValidationRequest),
    PostRegularVerification(OrderValidationRequest, UserAccountDetails),
    PreHookSim(OrderValidationRequest),
    PostPreHook(OrderValidationRequest, HashMap<Address, HashMap<U256, U256>>),
    PostHookSim(OrderValidationRequest, HashMap<Address, HashMap<U256, U256>>)
}

impl PipelineOperation for ValidationOperation {
    type End = ();

    fn get_next_operation(&self) -> u8 {
        match self {
            Self::PreRegularVerification(..) => 0,
            Self::PostRegularVerification(..) => 1,
            Self::PreHookSim(..) => 2,
            Self::PostPreHook(..) => 3,
            Self::PostHookSim(..) => 4
        }
    }
}

fn pre_regular_verification<DB>(
    item: ValidationOperation,
    cx: &mut ProcessingCtx<'_, DB>
) -> PipelineFut<ValidationOperation>
where
    DB: StateProviderFactory + Unpin + Clone + 'static
{
    Box::pin(std::future::ready({
        if let ValidationOperation::PreRegularVerification(verification) = item {
            let (res, details) = cx.state.validate_regular_order(verification);

            PipelineAction::Next(ValidationOperation::PostRegularVerification(res, details))
        } else {
            PipelineAction::Err
        }
    }))
}

fn post_regular_verification<DB>(
    item: ValidationOperation,
    cx: &mut ProcessingCtx<'_, DB>
) -> PipelineFut<ValidationOperation>
where
    DB: StateProviderFactory + Unpin + Clone + 'static
{
    if let ValidationOperation::PostRegularVerification(req, acc) = item {
        match req {
            OrderValidationRequest::ValidateLimit(a, b, c) => {
                let res = cx.user_orders.new_limit_order(c, deltas);
                let _ = a.send(res);
            }
            OrderValidationRequest::ValidateSearcher(a, b, c) => {
                let res = cx.user_orders.new_searcher_order(c, acc);
                let _ = a.send(res);
            }
            _ => unreachable!()
        }
    }

    Box::pin(std::future::ready({ PipelineAction::Return(()) }))
}

fn pre_hook_sim<DB>(
    item: ValidationOperation,
    cx: &mut ProcessingCtx<'_, DB>
) -> PipelineFut<ValidationOperation>
where
    DB: StateProviderFactory + Unpin + Clone + 'static
{
    Box::pin(std::future::ready({
        if let ValidationOperation::PreHookSim(sim) = item {
            let (a, b) = cx.sim.validate_pre_hook(sim);
            let (a, b) = cx.state.validate_state_prehook(a, b);
            PipelineAction::Next(ValidationOperation::PostPreHook(a, b))
        } else {
            PipelineAction::Err
        }
    }))
}

fn post_pre_hook_sim<DB>(
    item: ValidationOperation,
    cx: &mut ProcessingCtx<'_, DB>
) -> PipelineFut<ValidationOperation>
where
    DB: StateProviderFactory + Unpin + Clone + 'static
{
    if let ValidationOperation::PostPreHook(req, state) = item {
        let (a, b) = match req {
            OrderValidationRequest::ValidateComposableLimit(a, b, c) => {
                let res = cx.user_orders.new_limit_order(c, state);
            }
            OrderValidationRequest::ValidateComposableSearcher(a, b, c) => {
                let res = cx.user_orders.new_searcher_order(c, state);
            }
            _ => unreachable!()
        };

        Box::pin(std::future::ready({
            let (res, state) = cx.sim.validate_post_hook(a, b);
            let (a, b) = cx.state.validate_state_posthook(res, state);
            PipelineAction::Next(ValidationOperation::PostHookeSim(a, b))
        }))
    } else {
        Box::pin(std::future::ready({ PipelineAction::Err }))
    }
}

fn post_hook_sim<DB>(
    item: ValidationOperation,
    cx: &mut ProcessingCtx<'_, DB>
) -> PipelineFut<ValidationOperation>
where
    DB: StateProviderFactory + Unpin + Clone + 'static
{
    if let ValidationOperation::PostHookeSim(req, state) = item {
        match req {
            OrderValidationRequest::ValidateComposableLimit(a, b, c) => {
                let res = cx.user_orders.new_limit_order(c, state);
            }
            OrderValidationRequest::ValidateComposableSearcher(a, b, c) => {
                let res = cx.user_orders.new_searcher_order(c, state);
            }
            _ => unreachable!()
        };
        Box::pin(std::future::ready({ PipelineAction::Return(()) }))
    } else {
        Box::pin(std::future::ready({ PipelineAction::Err }))
    }
}
