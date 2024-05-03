use std::{
    pin::Pin,
    task::{Context, Poll}
};

use angstrom_types::orders::{OrderOrigin, PoolOrder, ValidationResults};
use futures_util::{stream::FuturesUnordered, Future, FutureExt, Stream, StreamExt};
use validation::order::OrderValidatorHandle;

type ValidationFuture<L, CL, S, CS> =
    Pin<Box<dyn Future<Output = ValidationResults<L, CL, S, CS>> + Send + Sync>>;

pub struct PoolOrderValidator<L, CL, S, CS, V>
where
    L: PoolOrder,
    CL: PoolOrder,
    S: PoolOrder,
    CS: PoolOrder,
    V: OrderValidatorHandle
{
    validator: V,
    pending:   FuturesUnordered<ValidationFuture<L, CL, S, CS>>
}

impl<L, CL, S, CS, V> PoolOrderValidator<L, CL, S, CS, V>
where
    L: PoolOrder,
    CL: PoolOrder,
    S: PoolOrder,
    CS: PoolOrder,
    V: OrderValidatorHandle<
        LimitOrder = L,
        SearcherOrder = S,
        ComposableLimitOrder = CL,
        ComposableSearcherOrder = CS
    >
{
    pub fn new(validator: V) -> Self {
        Self { validator, pending: FuturesUnordered::new() }
    }

    pub fn validate_order(&mut self, origin: OrderOrigin, order: L) {
        let val = self.validator.clone();
        self.pending.push(Box::pin(async move {
            val.validate_order(origin, order)
                .map(|res| ValidationResults::Limit(res))
                .await
        }) as ValidationFuture<_, _, _, _>);
    }

    pub fn validate_composable_order(&mut self, origin: OrderOrigin, order: CL) {
        let val = self.validator.clone();
        self.pending.push(Box::pin(async move {
            val.validate_composable_order(origin, order)
                .map(|res| ValidationResults::ComposableLimit(res))
                .await
        }));
    }

    pub fn validate_searcher_order(&mut self, origin: OrderOrigin, order: S) {
        let val = self.validator.clone();
        self.pending.push(Box::pin(async move {
            val.validate_searcher_order(origin, order)
                .map(|res| ValidationResults::Searcher(res))
                .await
        }) as ValidationFuture<_, _, _, _>);
    }

    pub fn validate_composable_searcher_order(&mut self, origin: OrderOrigin, order: CS) {
        let val = self.validator.clone();
        self.pending.push(Box::pin(async move {
            val.validate_composable_searcher_order(origin, order)
                .map(|res| ValidationResults::ComposableSearcher(res))
                .await
        }) as ValidationFuture<_, _, _, _>);
    }
}

impl<L, CL, S, CS, V> Stream for PoolOrderValidator<L, CL, S, CS, V>
where
    L: PoolOrder,
    CL: PoolOrder,
    S: PoolOrder,
    CS: PoolOrder,
    V: OrderValidatorHandle
{
    type Item = ValidationResults<L, CL, S, CS>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.pending.poll_next_unpin(cx)
    }
}
