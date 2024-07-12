use std::{
    pin::Pin,
    task::{Context, Poll}
};

use angstrom_types::{
    orders::OrderOrigin,
    sol_bindings::grouped_orders::{AllOrders, OrderWithStorageData}
};
use futures_util::{stream::FuturesUnordered, Future, Stream, StreamExt};
use validation::order::OrderValidatorHandle;

type ValidationFuture =
    Pin<Box<dyn Future<Output = OrderWithStorageData<AllOrders>> + Send + Sync>>;

pub struct PoolOrderValidator<V: OrderValidatorHandle> {
    validator: V,
    pending:   FuturesUnordered<ValidationFuture>
}

impl<V> PoolOrderValidator<V>
where
    V: OrderValidatorHandle<Order = AllOrders>
{
    pub fn new(validator: V) -> Self {
        Self { validator, pending: FuturesUnordered::new() }
    }

    pub fn validate_order(&mut self, origin: OrderOrigin, order: AllOrders) {
        let val = self.validator.clone();
        self.pending
            .push(Box::pin(async move { val.validate_order(origin, order).await }))
    }
}

impl<V> Stream for PoolOrderValidator<V>
where
    V: OrderValidatorHandle
{
    type Item = OrderWithStorageData<AllOrders>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.pending.poll_next_unpin(cx)
    }
}
