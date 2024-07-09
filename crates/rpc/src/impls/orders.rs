use alloy_primitives::Bytes;
use jsonrpsee::{core::RpcResult, PendingSubscriptionSink};
use order_pool::OrderPoolHandle;

use crate::{api::OrderApiServer, types::OrderSubscriptionKind};

pub struct OrderApi<OrderPool> {
    pub pool: OrderPool
}

#[async_trait::async_trait]
impl<OrderPool> OrderApiServer for OrderApi<OrderPool>
where
    OrderPool: OrderPoolHandle
{
    async fn submit_limit_order(&self, order: Bytes) -> RpcResult<bool> {
        // if let Ok(order) = order.try_into() {
        //     // self.pool.new_limit_order(OrderOrigin::External, order);
        //     Ok(true)
        // } else {
        //     Ok(false)
        // }
        Ok(true)
    }

    async fn submit_searcher_order(&self, order: Bytes) -> RpcResult<bool> {
        // if let Ok(order) = order.try_into() {
        //     self.pool.new_searcher_order(OrderOrigin::External, order);
        //     Ok(true)
        // } else {
        //     Ok(false)
        // }
        Ok(true)
    }

    async fn submit_composable_limit_order(&self, order: Bytes) -> RpcResult<bool> {
        // if let Ok(order) = order.try_into() {
        //     self.pool
        //         .new_composable_limit_order(OrderOrigin::External, order);
        //     Ok(true)
        // } else {
        //     Ok(false)
        // }
        Ok(true)
    }

    async fn subscribe_orders(
        &self,
        _pending: PendingSubscriptionSink,
        _kind: OrderSubscriptionKind
    ) -> jsonrpsee::core::SubscriptionResult {
        todo!()
    }
}
