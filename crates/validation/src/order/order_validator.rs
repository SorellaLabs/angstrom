use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    marker::PhantomData,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll}
};

use alloy_primitives::{Address, U256};
use angstrom_types::orders::{
    OrderValidationOutcome, PoolOrder, ValidatedOrder, ValidationResults
};
use angstrom_utils::sync_pipeline::{
    PipelineAction, PipelineBuilder, PipelineFut, PipelineOperation, PipelineWithIntermediary
};
use futures::{stream::FuturesUnordered, Future, StreamExt};
use futures_util::{future, FutureExt, Stream};
use reth_provider::StateProviderFactory;
use tokio::{runtime::Handle, task::JoinHandle};

use super::{
    sim::SimValidation,
    state::{
        config::ValidationConfig, orders::UserOrders, upkeepers::UserAccountDetails,
        StateValidation
    },
    OrderValidationRequest
};
use crate::{
    common::{executor::ThreadPool, lru_db::RevmLRU},
    order::sim,
    validator::ValidationRequest
};

#[allow(dead_code)]
pub struct OrderValidator<'a, DB> {
    sim:      SimValidation<DB>,
    state:    StateValidation<DB>,
    orders:   UserOrders,
    pipeline: PipelineWithIntermediary<Handle, ValidationOperation, ProcessingCtx<'a, DB>>,
    _p:       PhantomData<&'a u8>
}

impl<'this, DB> OrderValidator<'this, DB>
where
    DB: StateProviderFactory + Unpin + Clone + 'static
{
    pub fn new(db: Arc<RevmLRU<DB>>, config: ValidationConfig) -> Self {
        let state = StateValidation::new(db.clone(), config);
        let sim = SimValidation::new(db);

        let new_state = state.clone();
        let new_sim = sim.clone();

        let pipeline = PipelineBuilder::new()
            .add_step(0, ValidationOperation::pre_regular_verification)
            .add_step(1, ValidationOperation::post_regular_verification)
            .add_step(2, ValidationOperation::pre_hook_sim)
            .add_step(3, ValidationOperation::post_pre_hook_sim)
            .add_step(4, ValidationOperation::post_hook_sim)
            .build(tokio::runtime::Handle::current());

        Self { state, sim, pipeline, orders: UserOrders::new(), _p: PhantomData }
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

impl<DB> Future for OrderValidator<'_, DB>
where
    DB: StateProviderFactory + Clone + Unpin + 'static
{
    type Output = ();

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>
    ) -> std::task::Poll<Self::Output> {
        let state = self.state.clone();
        let sim = self.sim.clone();
        let mut ctx = ProcessingCtx::new(&mut self.orders as *mut UserOrders, sim, state);

        while let Poll::Ready(Some(_)) = self.pipeline.poll(cx, &mut ctx) {}

        Poll::Pending
    }
}

/// represents different steps in the validation process that we want to run on
/// its own task
pub enum ValidationOperation {
    PreRegularVerification(OrderValidationRequest),
    PostRegularVerification(OrderValidationRequest, UserAccountDetails),
    PreHookSim(OrderValidationRequest),
    PostPreHook(OrderValidationRequest, UserAccountDetails, HashMap<Address, HashMap<U256, U256>>),
    PostHookSim(OrderValidationRequest, UserAccountDetails)
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

pub struct ProcessingCtx<'a, DB> {
    user_orders: *mut UserOrders,
    pub sim:     SimValidation<DB>,
    pub state:   StateValidation<DB>,
    _p:          PhantomData<&'a u8>
}

impl<'a, DB> ProcessingCtx<'a, DB> {
    pub fn new(
        user_orders: *mut UserOrders,
        sim: SimValidation<DB>,
        state: StateValidation<DB>
    ) -> Self {
        Self { sim, user_orders, state, _p: PhantomData::default() }
    }

    pub fn user_orders(&mut self) -> &'a mut UserOrders {
        unsafe { &mut (*self.user_orders) }
    }
}

impl ValidationOperation {
    fn pre_regular_verification<DB>(self, cx: &mut ProcessingCtx<DB>) -> PipelineFut<Self>
    where
        DB: StateProviderFactory + Unpin + Clone + 'static
    {
        Box::pin(std::future::ready({
            if let ValidationOperation::PreRegularVerification(verification) = self {
                let (res, details) = cx.state.validate_regular_order(verification);

                PipelineAction::Next(ValidationOperation::PostRegularVerification(res, details))
            } else {
                PipelineAction::Err
            }
        }))
    }

    fn post_regular_verification<DB>(self, cx: &mut ProcessingCtx<DB>) -> PipelineFut<Self>
    where
        DB: StateProviderFactory + Unpin + Clone + 'static
    {
        if let ValidationOperation::PostRegularVerification(req, deltas) = self {
            match req {
                OrderValidationRequest::ValidateLimit(a, b, c) => {
                    let res = cx.user_orders().new_limit_order(c, deltas);
                    let _ = a.send(res);
                }
                OrderValidationRequest::ValidateSearcher(a, b, c) => {
                    let res = cx.user_orders().new_searcher_order(c, deltas);
                    let _ = a.send(res);
                }
                _ => unreachable!()
            }
        }

        Box::pin(std::future::ready(PipelineAction::Return(())))
    }

    fn pre_hook_sim<DB>(self, cx: &mut ProcessingCtx<DB>) -> PipelineFut<Self>
    where
        DB: StateProviderFactory + Unpin + Clone + 'static
    {
        Box::pin(std::future::ready({
            if let ValidationOperation::PreHookSim(sim) = self {
                let (req, overrides) = cx.sim.validate_pre_hook(sim);
                let (req, details) = cx.state.validate_state_prehook(req, &overrides);
                PipelineAction::Next(ValidationOperation::PostPreHook(req, details, overrides))
            } else {
                PipelineAction::Err
            }
        }))
    }

    fn post_pre_hook_sim<DB>(self, cx: &mut ProcessingCtx<DB>) -> PipelineFut<Self>
    where
        DB: StateProviderFactory + Unpin + Clone + 'static
    {
        if let ValidationOperation::PostPreHook(req, acc_details, state) = self {
            let (order, overrides) = match req {
                OrderValidationRequest::ValidateComposableLimit(tx, origin, order) => {
                    let (order, overrides) = cx
                        .user_orders()
                        .new_composable_limit_order(order, acc_details);
                    if let OrderValidationOutcome::Valid { order, propagate } = order {
                        (
                            OrderValidationRequest::ValidateComposableLimit(
                                tx,
                                origin,
                                order.order
                            ),
                            overrides
                        )
                    } else {
                        return Box::pin(std::future::ready(PipelineAction::Err))
                    }
                }
                OrderValidationRequest::ValidateComposableSearcher(tx, origin, order) => {
                    let (order, overrides) = cx
                        .user_orders()
                        .new_composable_searcher_order(order, acc_details);

                    if let OrderValidationOutcome::Valid { order, propagate } = order {
                        (
                            OrderValidationRequest::ValidateComposableSearcher(
                                tx,
                                origin,
                                order.order
                            ),
                            overrides
                        )
                    } else {
                        return Box::pin(std::future::ready(PipelineAction::Err))
                    }
                }
                _ => unreachable!()
            };

            Box::pin(std::future::ready({
                let (res, state) = cx.sim.validate_post_hook(order, overrides);
                let (res, user_deltas) = cx.state.validate_state_posthook(res, &state);
                PipelineAction::Next(ValidationOperation::PostHookSim(res, user_deltas))
            }))
        } else {
            Box::pin(std::future::ready(PipelineAction::Err))
        }
    }

    fn post_hook_sim<DB>(self, cx: &mut ProcessingCtx<DB>) -> PipelineFut<Self>
    where
        DB: StateProviderFactory + Unpin + Clone + 'static
    {
        if let ValidationOperation::PostHookSim(req, user_deltas) = self {
            match req {
                OrderValidationRequest::ValidateComposableLimit(tx, origin, order) => {
                    let (res, _) = cx
                        .user_orders()
                        .new_composable_limit_order(order, user_deltas);
                    let _ = tx.send(res);
                }
                OrderValidationRequest::ValidateComposableSearcher(tx, origin, order) => {
                    let (res, _) = cx
                        .user_orders()
                        .new_composable_searcher_order(order, user_deltas);
                    let _ = tx.send(res);
                }
                _ => unreachable!()
            };
            Box::pin(std::future::ready(PipelineAction::Return(())))
        } else {
            Box::pin(std::future::ready(PipelineAction::Err))
        }
    }
}
