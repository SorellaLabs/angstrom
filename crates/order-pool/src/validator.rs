use std::{
    collections::VecDeque,
    pin::Pin,
    task::{Context, Poll}
};

use angstrom_types::{orders::OrderOrigin, sol_bindings::grouped_orders::AllOrders};
use futures_util::{stream::FuturesUnordered, Future, FutureExt, Stream, StreamExt};
use reth_primitives::{Address, B256};
use tracing::info;
use validation::order::{OrderValidationResults, OrderValidatorHandle};

type ValidationFuture = Pin<Box<dyn Future<Output = OrderValidationResults> + Send + Sync>>;

pub enum OrderValidator<V: OrderValidatorHandle> {
    /// Waits for all current processing to be completed. This allows us
    /// to have all orders for the previous block be indexed properly so that
    /// when we go to re-validate everything, there isn't a order that
    /// was validated against block n -1 when we are on block n where there
    /// was some state transition on the address
    ClearingForNewBlock {
        validator:              V,
        block_number:           u64,
        waiting_for_new_block:  VecDeque<(OrderOrigin, AllOrders)>,
        /// all order hashes that have been filled or expired.
        completed_orders:       Vec<B256>,
        /// all addresses that we need to invalidate the cache for balances /
        /// approvals
        revalidation_addresses: Vec<Address>,
        remaining_futures:      FuturesUnordered<ValidationFuture>
    },
    /// The inform state is telling the validation client to
    /// progress a block and the cache segments it should remove + pending order
    /// state that no longer exists. Once this is done, we can be assured that
    /// the order validator has the correct state and thus can progress.
    InformState {
        validator:             V,
        waiting_for_new_block: VecDeque<(OrderOrigin, AllOrders)>,
        future:                ValidationFuture
    },
    RegularProcessing {
        validator:         V,
        remaining_futures: FuturesUnordered<ValidationFuture>
    }
}

impl<V> OrderValidator<V>
where
    V: OrderValidatorHandle<Order = AllOrders>
{
    pub fn new(validator: V) -> Self {
        Self::RegularProcessing { validator, remaining_futures: FuturesUnordered::new() }
    }

    pub fn on_new_block(
        &mut self,
        block_number: u64,
        completed_orders: Vec<B256>,
        revalidation_addresses: Vec<Address>
    ) {
        assert!(
            !self.is_transitioning(),
            "already clearing for new block. if this gets triggered, means we have a big runtime \
             issue"
        );
        let Self::RegularProcessing { validator, remaining_futures } = self else { unreachable!() };

        // only good way to move data over
        let rem_futures = unsafe { std::ptr::read(remaining_futures) };

        *self = Self::ClearingForNewBlock {
            validator: validator.clone(),
            waiting_for_new_block: VecDeque::default(),
            remaining_futures: rem_futures,
            completed_orders,
            revalidation_addresses,
            block_number
        };
    }

    pub fn validate_order(&mut self, origin: OrderOrigin, order: AllOrders) {
        match self {
            Self::RegularProcessing { remaining_futures, validator } => {
                let val = validator.clone();
                remaining_futures
                    .push(Box::pin(async move { val.validate_order(origin, order).await }))
            }
            Self::ClearingForNewBlock { waiting_for_new_block, .. } => {
                waiting_for_new_block.push_back((origin, order));
            }
            Self::InformState { waiting_for_new_block, .. } => {
                waiting_for_new_block.push_back((origin, order));
            }
        }
    }

    fn is_transitioning(&self) -> bool {
        matches!(self, Self::ClearingForNewBlock { .. } | Self::InformState { .. })
    }

    fn handle_inform(
        validator: &mut V,
        waiting_for_new_block: &mut VecDeque<(OrderOrigin, AllOrders)>,
        future: &mut ValidationFuture,
        cx: &mut Context<'_>
    ) -> Option<Self> {
        if let Poll::Ready(_) = future.poll_unpin(cx) {
            // lfg we have finished validating.
            let validator_clone = validator.clone();
            let mut this = Self::RegularProcessing {
                validator:         validator_clone,
                remaining_futures: FuturesUnordered::default()
            };
            waiting_for_new_block.drain(..).for_each(|(origin, order)| {
                this.validate_order(origin, order);
            });

            return Some(this)
        }

        None
    }
}

impl<V: OrderValidatorHandle> Stream for OrderValidator<V>
where
    V: OrderValidatorHandle<Order = AllOrders>
{
    type Item = OrderValidatorRes;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        match this {
            OrderValidator::ClearingForNewBlock {
                validator,
                block_number,
                waiting_for_new_block,
                completed_orders,
                revalidation_addresses,
                remaining_futures
            } => {
                let next = remaining_futures.poll_next_unpin(cx);
                match next {
                    res @ Poll::Ready(Some(_)) => {
                        return res.map(|inner| inner.map(|v| OrderValidatorRes::ValidatedOrder(v)))
                    }
                    Poll::Pending => return Poll::Pending,
                    _ => {}
                }
                info!(
                    "clearing for new block done. triggering clearing and starting to validate \
                     state for current block"
                );
                let v = validator.clone();
                let emit_completed = completed_orders.clone();
                let emit_address = revalidation_addresses.clone();
                let completed_orders = std::mem::take(completed_orders);
                let revalidation_addresses = std::mem::take(revalidation_addresses);
                let bn = *block_number;

                let fut = Box::pin(async move {
                    v.new_block(bn, completed_orders, revalidation_addresses)
                        .await
                });

                *this = Self::InformState {
                    validator:             validator.clone(),
                    waiting_for_new_block: std::mem::take(waiting_for_new_block),
                    future:                fut
                };

                Poll::Ready(Some(OrderValidatorRes::EnsureClearForTransition {
                    orders:    emit_completed,
                    addresses: emit_address
                }))
            }
            OrderValidator::InformState { validator, waiting_for_new_block, future } => {
                let Some(new_state) =
                    Self::handle_inform(validator, waiting_for_new_block, future, cx)
                else {
                    return Poll::Pending
                };
                *this = new_state;
                cx.waker().wake_by_ref();
                return Poll::Pending
            }
            OrderValidator::RegularProcessing { remaining_futures, .. } => remaining_futures
                .poll_next_unpin(cx)
                .map(|inner| inner.map(|v| OrderValidatorRes::ValidatedOrder(v)))
        }
    }
}

pub enum OrderValidatorRes {
    /// standard flow
    ValidatedOrder(OrderValidationResults),
    /// Once all orders for the previous block have been validated. we go
    /// through all the addresses and orders and cleanup. once this is done
    /// we can go back to general flow.
    EnsureClearForTransition { orders: Vec<B256>, addresses: Vec<Address> }
}

// pub struct PoolOrderValidator<V: OrderValidatorHandle> {
//     validator:       V,
//     /// when a new block is detected, we need to wait for all current
//     /// validation, processes to complete before we can transition to
//     /// validating new blocks.
//     waiting_clear:   bool,
//     new_block_queue: VecDeque<(OrderOrigin, AllOrders)>,
//     pending:         FuturesUnordered<ValidationFuture>
// }
//
// impl<V> PoolOrderValidator<V>
// where
//     V: OrderValidatorHandle<Order = AllOrders>
// {
//     pub fn new(validator: V) -> Self {
//         Self {
//             validator,
//             pending: FuturesUnordered::new(),
//             new_block_queue: VecDeque::default(),
//             waiting_clear: false
//         }
//     }
//
//     pub fn validation_has_cleared(&self) -> bool {
//         !self.waiting_clear
//     }
//
//     pub fn validate_order(&mut self, origin: OrderOrigin, order: AllOrders) {
//         if self.waiting_clear {
//             self.new_block_queue.push_back((origin, order));
//             return
//         }
//
//         let val = self.validator.clone();
//         self.pending
//             .push(Box::pin(async move { val.validate_order(origin,
// order).await }))     }
// }
//
// impl<V> Stream for PoolOrderValidator<V>
// where
//     V: OrderValidatorHandle
// {
//     type Item = OrderValidationResults;
//
//     fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) ->
// Poll<Option<Self::Item>> {         let result =
// self.pending.poll_next_unpin(cx);
//
//         // mark as ready to process
//         if self.pending.is_empty() && self.waiting_clear {
//             self.waiting_clear = false;
//         }
//
//         result
//     }
// }
