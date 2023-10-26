use guard_types::rpc::{
    EcRecoveredComposableLimitOrder, EcRecoveredComposableSearcherOrder, EcRecoveredLimitOrder,
    EcRecoveredSearcherOrder, SignedComposableLimitOrder, SignedComposableSearcherOrder,
    SignedLimitOrder, SignedSearcherOrder
};
use jsonrpsee::{core::RpcResult, PendingSubscriptionSink};

use crate::{
    api::{OrderApiServer, OrderPubSubApiServer},
    types::OrderSubscriptionKind
};

pub struct OrderApi<OrderPool> {
    order: OrderPool
}

#[async_trait::async_trait]
impl<OrderPool> OrderApiServer for OrderApi<OrderPool>
where
    OrderPool: Send + Sync + 'static
{
    async fn submit_limit_order(&self, order: SignedLimitOrder) -> RpcResult<bool> {
        if let Ok(order) = order.try_into() {
            let order: EcRecoveredLimitOrder = order;
            todo!()
        } else {
            Ok(false)
        }
    }

    async fn submit_searcher_order(&self, order: SignedSearcherOrder) -> RpcResult<bool> {
        if let Ok(order) = order.try_into() {
            let order: EcRecoveredSearcherOrder = order;
            todo!()
        } else {
            Ok(false)
        }
    }

    async fn submit_composable_limit_order(
        &self,
        order: SignedComposableLimitOrder
    ) -> RpcResult<bool> {
        if let Ok(order) = order.try_into() {
            let order: EcRecoveredComposableLimitOrder = order;
            todo!()
        } else {
            Ok(false)
        }
    }

    async fn submit_composable_searcher_order(
        &self,
        order: SignedComposableSearcherOrder
    ) -> RpcResult<bool> {
        if let Ok(order) = order.try_into() {
            let order: EcRecoveredComposableSearcherOrder = order;
            todo!()
        } else {
            Ok(false)
        }
    }
}

#[async_trait::async_trait]
impl<OrderPool> OrderPubSubApiServer for OrderApi<OrderPool>
where
    OrderPool: Send + Sync + 'static
{
    async fn subscribe(
        &self,
        pending: PendingSubscriptionSink,
        kind: OrderSubscriptionKind
    ) -> jsonrpsee::core::SubscriptionResult {
        todo!()
    }
}
