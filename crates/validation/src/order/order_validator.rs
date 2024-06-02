use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    marker::PhantomData,
    pin::Pin,
    sync::{atomic::AtomicU64, Arc},
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
    common::{
        executor::ThreadPool,
        lru_db::{BlockStateProviderFactory, RevmLRU}
    },
    order::sim,
    validator::ValidationRequest
};

pub struct OrderValidator<DB> {
    sim:          SimValidation<DB>,
    state:        StateValidation<DB>,
    orders:       UserOrders,
    pipeline:     PipelineWithIntermediary<Handle, ValidationOperation, ProcessingCtx<DB>>,
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

        let pipeline = PipelineBuilder::new()
            .add_step(0, ValidationOperation::pre_regular_verification)
            .add_step(1, ValidationOperation::post_regular_verification)
            .add_step(2, ValidationOperation::pre_hook_sim)
            .add_step(3, ValidationOperation::post_pre_hook_sim)
            .add_step(4, ValidationOperation::post_hook_sim)
            .build(tokio::runtime::Handle::current());

        Self { state, sim, pipeline, orders: UserOrders::new(), block_number }
    }

    pub fn update_block_number(&mut self, number: u64) {
        self.block_number
            .store(number, std::sync::atomic::Ordering::SeqCst);
    }

    /// only checks state
    pub fn validate_order(&mut self, order: OrderValidationRequest) {
        let block_number = self.block_number.load(std::sync::atomic::Ordering::SeqCst);
        match order {
            order @ OrderValidationRequest::ValidateLimit(..) => {
                self.pipeline
                    .add(ValidationOperation::PreRegularVerification(order, block_number));
            }
            order @ OrderValidationRequest::ValidateSearcher(..) => self
                .pipeline
                .add(ValidationOperation::PreRegularVerification(order, block_number)),

            order @ OrderValidationRequest::ValidateComposableLimit(..) => self
                .pipeline
                .add(ValidationOperation::PreHookSim(order, block_number)),

            order @ OrderValidationRequest::ValidateComposableSearcher(..) => self
                .pipeline
                .add(ValidationOperation::PreHookSim(order, block_number))
        }
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

/// represents different steps in the validation process that we want to run on
/// its own task
pub enum ValidationOperation {
    PreRegularVerification(OrderValidationRequest, u64),
    PostRegularVerification(OrderValidationRequest, UserAccountDetails, u64),
    PreHookSim(OrderValidationRequest, u64),
    PostPreHook(
        OrderValidationRequest,
        UserAccountDetails,
        HashMap<Address, HashMap<U256, U256>>,
        u64
    ),
    PostHookSim(OrderValidationRequest, UserAccountDetails, u64)
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

pub struct ProcessingCtx<DB> {
    user_orders:              *mut UserOrders,
    pub sim:                  SimValidation<DB>,
    pub state:                StateValidation<DB>,
    pub current_block_number: Arc<AtomicU64>
}

impl<DB> ProcessingCtx<DB> {
    pub fn new(
        user_orders: *mut UserOrders,
        sim: SimValidation<DB>,
        state: StateValidation<DB>,
        current_block_number: Arc<AtomicU64>
    ) -> Self {
        Self { sim, user_orders, state, current_block_number }
    }

    pub fn user_orders<'a>(&'a mut self) -> &'a mut UserOrders {
        unsafe { &mut (*self.user_orders) }
    }
}

impl ValidationOperation {
    fn pre_regular_verification<DB>(self, cx: &mut ProcessingCtx<DB>) -> PipelineFut<Self>
    where
        DB: BlockStateProviderFactory + Unpin + Clone + 'static
    {
        let cur_block = cx.current_block_number.clone();
        Box::pin(std::future::ready({
            if let ValidationOperation::PreRegularVerification(verification, block_number) = self {
                let (res, details) = cx.state.validate_regular_order(verification);

                // check if the block_number has changed durning validation. If it has, then we
                // return to the first step in order to ensure proper state has been loaded
                let cur_block = cur_block.load(std::sync::atomic::Ordering::SeqCst);
                if cur_block != block_number {
                    PipelineAction::Next(ValidationOperation::PreRegularVerification(
                        res, cur_block
                    ))
                } else {
                    PipelineAction::Next(ValidationOperation::PostRegularVerification(
                        res,
                        details,
                        block_number
                    ))
                }
            } else {
                PipelineAction::Err
            }
        }))
    }

    fn post_regular_verification<DB>(self, cx: &mut ProcessingCtx<DB>) -> PipelineFut<Self>
    where
        DB: BlockStateProviderFactory + Unpin + Clone + 'static
    {
        if let ValidationOperation::PostRegularVerification(req, deltas, block_number) = self {
            match req {
                OrderValidationRequest::ValidateLimit(a, b, c) => {
                    let res = cx.user_orders().new_limit_order(c, deltas, block_number);
                    let _ = a.send(res);
                }
                OrderValidationRequest::ValidateSearcher(a, b, c) => {
                    let res = cx.user_orders().new_searcher_order(c, deltas, block_number);
                    let _ = a.send(res);
                }
                _ => unreachable!()
            }
        }

        Box::pin(std::future::ready(PipelineAction::Return(())))
    }

    fn pre_hook_sim<DB>(self, cx: &mut ProcessingCtx<DB>) -> PipelineFut<Self>
    where
        DB: BlockStateProviderFactory + Unpin + Clone + 'static
    {
        let cur_block = cx.current_block_number.clone();
        Box::pin(std::future::ready({
            if let ValidationOperation::PreHookSim(sim, block_number) = self {
                let (req, overrides) = cx.sim.validate_pre_hook(sim);
                let (req, details) = cx.state.validate_state_prehook(req, &overrides);

                // if block number change, we reset and retest the state
                let cur_block = cur_block.load(std::sync::atomic::Ordering::SeqCst);
                if cur_block != block_number {
                    PipelineAction::Next(ValidationOperation::PreHookSim(req, cur_block))
                } else {
                    PipelineAction::Next(ValidationOperation::PostPreHook(
                        req,
                        details,
                        overrides,
                        block_number
                    ))
                }
            } else {
                PipelineAction::Err
            }
        }))
    }

    fn post_pre_hook_sim<DB>(self, cx: &mut ProcessingCtx<DB>) -> PipelineFut<Self>
    where
        DB: BlockStateProviderFactory + Unpin + Clone + 'static
    {
        if let ValidationOperation::PostPreHook(req, acc_details, state, block_number) = self {
            let (order, overrides, block_number) = match req {
                OrderValidationRequest::ValidateComposableLimit(tx, origin, order) => {
                    let (order, overrides) = cx.user_orders().new_composable_limit_order(
                        order,
                        acc_details,
                        block_number
                    );
                    if let OrderValidationOutcome::Valid { order, propagate, block_number } = order
                    {
                        (
                            OrderValidationRequest::ValidateComposableLimit(
                                tx,
                                origin,
                                order.order
                            ),
                            overrides,
                            block_number
                        )
                    } else {
                        return Box::pin(std::future::ready(PipelineAction::Err))
                    }
                }

                OrderValidationRequest::ValidateComposableSearcher(tx, origin, order) => {
                    let (order, overrides) = cx.user_orders().new_composable_searcher_order(
                        order,
                        acc_details,
                        block_number
                    );

                    if let OrderValidationOutcome::Valid { order, propagate, block_number } = order
                    {
                        (
                            OrderValidationRequest::ValidateComposableSearcher(
                                tx,
                                origin,
                                order.order
                            ),
                            overrides,
                            block_number
                        )
                    } else {
                        return Box::pin(std::future::ready(PipelineAction::Err))
                    }
                }
                _ => unreachable!()
            };

            let cur_block = cx.current_block_number.clone();

            Box::pin(std::future::ready({
                let (res, state) = cx.sim.validate_post_hook(order, overrides);
                let (res, user_deltas) = cx.state.validate_state_posthook(res, &state);

                // ensure we had proper validation on the post hook
                let cur_block = cur_block.load(std::sync::atomic::Ordering::SeqCst);
                if cur_block != block_number {
                    PipelineAction::Next(ValidationOperation::PreHookSim(res, cur_block))
                } else {
                    PipelineAction::Next(ValidationOperation::PostHookSim(
                        res,
                        user_deltas,
                        block_number
                    ))
                }
            }))
        } else {
            Box::pin(std::future::ready(PipelineAction::Err))
        }
    }

    fn post_hook_sim<DB>(self, cx: &mut ProcessingCtx<DB>) -> PipelineFut<Self>
    where
        DB: BlockStateProviderFactory + Unpin + Clone + 'static
    {
        if let ValidationOperation::PostHookSim(req, user_deltas, block_number) = self {
            match req {
                OrderValidationRequest::ValidateComposableLimit(tx, origin, order) => {
                    let (res, _) = cx.user_orders().new_composable_limit_order(
                        order,
                        user_deltas,
                        block_number
                    );
                    let _ = tx.send(res);
                }
                OrderValidationRequest::ValidateComposableSearcher(tx, origin, order) => {
                    let (res, _) = cx.user_orders().new_composable_searcher_order(
                        order,
                        user_deltas,
                        block_number
                    );
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
