use std::{
    collections::VecDeque,
    marker::PhantomData,
    task::{Context, Poll}
};

use alloy_primitives::{Address, U256};
use futures::{stream::FuturesUnordered, Future, StreamExt};
use futures_util::{FutureExt, Stream};
use guard_types::orders::{OrderValidationOutcome, PoolOrder, ValidatedOrder, ValidationResults};
use guard_utils::sync_pipeline::{PipelineBuilder, PipelineOperation, PipelineWithIntermediary};
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
    validator::ValidationRequest
};

pub enum ValidationOperation {
    RegularVerification { request: OrderValidationRequest, details: UserAccountDetails },
    PreHookSim(OrderValidationRequest),
    PostPreHook(OrderValidationRequest, HashMap<Address, HashMap<U256, U256>>),
    PostHookSim(OrderValidationRequest, HashMap<Address, HashMap<U256, U256>>),
    PostPostHookeSim(OrderValidationRequest, HashMap<Address, HashMap<U256, U256>>)
}

impl PipelineOperation for ValidationOperation {
    type End = ();

    fn get_next_operation(&self) -> u8 {
        match self {
            Self::RegularVerification { .. } => 0,
            Self::PreHookSim(..) => 1,
            Self::PostPreHook(..) => 2,
            Self::PostHookSim(..) => 3,
            Self::PostPostHookeSim(..) => 4
        }
    }
}

#[allow(dead_code)]
pub struct OrderValidator<DB> {
    sim:     SimValidation<DB>,
    state:   StateValidation<DB>,
    orders:  UserOrders,

    pipline: PipelineWithIntermediary<Handle, ValidationOperation, UserOrders>,
}

impl<DB> OrderValidator<DB>
where
    DB: StateProviderFactory + Unpin + 'static
{
    pub fn new() -> Self {
        todo!()

    }
    /// only checks state
    pub fn validate_order(&mut self, order: OrderValidationRequest) {
        // match order {
        //     order @ OrderValidationRequest::ValidateLimit(..)
        //     | OrderValidationRequest::ValidateSearcher(..) => self
        //         .thread_pool
        //         .spawn_return_task_as(async {
        // self.state.validate_regular_order(order) })
        //         .map(|item| {
        //             if let Ok((val, other)) = item {
        //                 self.on_task_resolve(val, other)
        //             }
        //         }),
        //     order @ OrderValidationRequest::ValidateComposableLimit(..)
        //     | OrderValidationRequest::ValidateComposableSearcher(..) => self
        //         .thread_pool
        //         .spawn_return_task_as(async {
        //             let state = self.sim.validate_pre_hook(order);
        //             // do shit with ordres
        //             // self.orders.
        //         })
        //         .map(|res| async {
        //             self.thread_pool
        //                 .spawn_return_task_as(async move {
        //                     res.map(|(a, b)|
        // self.state.validate_state_prehook(a, b))                 })
        //                 .await
        //         })
        //         .map(|a| async {
        //             let res = a.await.map(|e| e.map(|e|{
        //                 self.sim.validate_post_hook(order)
        //             });
        //         })
        // }
    }

    fn on_task_resolve(&mut self, request: OrderValidationRequest, details: UserAccountDetails) {
        // match request {
        //     OrderValidationRequest::ValidateLimit(tx, origin, order) => {
        //         let result = self.user_orders.new_limit_order(order,
        // details);         let _ = tx.send(result);
        //     }
        //     OrderValidationRequest::ValidateSearcher(tx, origin, order) => {
        //         let result = self.user_orders.new_searcher_order(order,
        // details);         let _ = tx.send(result);
        //     }
        //     OrderValidationRequest::ValidateComposableLimit(tx, origin,
        // order) => {         let result =
        // self.user_orders.new_composable_limit_order(order, deltas);
        //         if !result.is_valid() {
        //             let _ = tx.send(result);
        //             return None
        //         }
        //         return Some(OrderValidationRequest::ValidateComposableLimit(
        //             tx,
        //             origin,
        //             result.try_get_order().unwrap()
        //         ))
        //     }
        //     OrderValidationRequest::ValidateComposableSearcher(tx, origin,
        // order) => {         let result =
        // self.user_orders.new_composable_sercher_order(order, deltas);
        //         if !result.is_valid() {
        //             let _ = tx.send(result);
        //             return None
        //         }
        //
        //         return Some(OrderValidationRequest::ValidateComposableLimit(
        //             tx,
        //             origin,
        //             result.try_get_order().unwrap()
        //         ))
        //     }
        // }
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

        Poll::Pending
    }
}
