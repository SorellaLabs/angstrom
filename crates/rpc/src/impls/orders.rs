use angstrom_types::{
    orders::OrderOrigin,
    sol_bindings::{
        grouped_orders::{AllOrders, FlashVariants, StandingVariants},
        rpc_orders::{
            ExactFlashOrder, ExactStandingOrder, PartialFlashOrder, PartialStandingOrder,
            TopOfBlockOrder
        }
    }
};
use jsonrpsee::{core::RpcResult, PendingSubscriptionSink, SubscriptionMessage};
use order_pool::{OrderPoolHandle, PoolManagerUpdate};
use reth_tasks::TaskSpawner;

use crate::{
    api::OrderApiServer,
    types::{OrderSubscriptionKind, OrderSubscriptionResult}
};

pub struct OrderApi<OrderPool, Spawner> {
    pool:             OrderPool,
    task_spawner:     Spawner,
}

impl<OrderPool, Spawner> OrderApi<OrderPool, Spawner> {
    pub fn new(pool: OrderPool, task_spawner: Spawner) -> Self {
        Self {
            pool,
            task_spawner,
        }
    }
}

#[async_trait::async_trait]
impl<OrderPool, Spawner> OrderApiServer for OrderApi<OrderPool, Spawner>
where
    OrderPool: OrderPoolHandle,
    Spawner: TaskSpawner + 'static
{
    async fn send_partial_standing_order(&self, order: PartialStandingOrder) -> RpcResult<bool> {
        let order = AllOrders::Standing(StandingVariants::Partial(order));
        Ok(self.pool.new_order(OrderOrigin::External, order).await)
    }

    async fn send_exact_standing_order(&self, order: ExactStandingOrder) -> RpcResult<bool> {
        let order = AllOrders::Standing(StandingVariants::Exact(order));
        Ok(self.pool.new_order(OrderOrigin::External, order).await)
    }

    async fn send_searcher_order(&self, order: TopOfBlockOrder) -> RpcResult<bool> {
        let order = AllOrders::TOB(order);
        Ok(self.pool.new_order(OrderOrigin::External, order).await)
    }

    async fn send_partial_flash_order(&self, order: PartialFlashOrder) -> RpcResult<bool> {
        let order = AllOrders::Flash(FlashVariants::Partial(order));
        Ok(self.pool.new_order(OrderOrigin::External, order).await)
    }

    async fn send_exact_flash_order(&self, order: ExactFlashOrder) -> RpcResult<bool> {
        let order = AllOrders::Flash(FlashVariants::Exact(order));
        Ok(self.pool.new_order(OrderOrigin::External, order).await)
    }

    async fn subscribe_orders(
        &self,
        pending: PendingSubscriptionSink,
        kind: OrderSubscriptionKind
    ) -> jsonrpsee::core::SubscriptionResult {
        let sink = pending.accept().await?;
        let mut subscription = self.pool.subscribe_orders();

        self.task_spawner.spawn(Box::pin(async move {
            while let Ok(order) = subscription.recv().await {
                if sink.is_closed() {
                    break;
                }

                let msg = match (&kind, order) {
                    (OrderSubscriptionKind::NewOrders, PoolManagerUpdate::NewOrder(order_update)) => {
                        Some(OrderSubscriptionResult::NewOrder(order_update))
                    },
                    (OrderSubscriptionKind::FilledOrders, PoolManagerUpdate::FilledOrder((block_number, filled_order))) => {
                        Some(OrderSubscriptionResult::FilledOrder((block_number, filled_order)))
                    },
                    (OrderSubscriptionKind::UnfilleOrders, PoolManagerUpdate::UnfilledOrders(unfilled_order)) => {
                        Some(OrderSubscriptionResult::UnfilledOrder(unfilled_order))
                    },
                    (OrderSubscriptionKind::NewOrders, PoolManagerUpdate::FilledOrder(_)) => None,
                    (OrderSubscriptionKind::NewOrders, PoolManagerUpdate::UnfilledOrders(_)) => None,
                    (OrderSubscriptionKind::FilledOrders, PoolManagerUpdate::NewOrder(_)) => None,
                    (OrderSubscriptionKind::FilledOrders, PoolManagerUpdate::UnfilledOrders(_)) => None,
                    (OrderSubscriptionKind::UnfilleOrders, PoolManagerUpdate::NewOrder(_)) => None,
                    (OrderSubscriptionKind::UnfilleOrders, PoolManagerUpdate::FilledOrder(_)) => None,
                };

                if let Some(result) = msg {
                    match SubscriptionMessage::from_json(&result) {
                        Ok(message) => {
                            if sink.send(message).await.is_err() {
                                break;
                            }
                        },
                        Err(e) => {
                            tracing::error!("Failed to serialize subscription message: {:?}", e);
                        }
                    }
                }
            }
        }));

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{future, future::Future};

    use angstrom_network::pool_manager::OrderCommand;
    use angstrom_types::sol_bindings::rpc_orders::{
        ExactFlashOrder, ExactStandingOrder, PartialFlashOrder, PartialStandingOrder,
        TopOfBlockOrder
    };
    use order_pool::PoolManagerUpdate;
    use reth_tasks::TokioTaskExecutor;
    use tokio::sync::{
        broadcast::Receiver,
        mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender}
    };

    use super::*;

    #[tokio::test]
    async fn test_send_partial_standing_order() {
        let (_handle, api) = setup_order_api();
        let order = PartialStandingOrder::default();
        assert!(api
            .send_partial_standing_order(order)
            .await
            .expect("to not throw error"));
    }

    #[tokio::test]
    async fn test_send_exact_standing_order() {
        let (_handle, api) = setup_order_api();
        let order = ExactStandingOrder::default();
        assert!(api
            .send_exact_standing_order(order)
            .await
            .expect("to not throw error"));
    }

    #[tokio::test]
    async fn test_send_searcher_order() {
        let (_handle, api) = setup_order_api();
        let order = TopOfBlockOrder::default();
        assert!(api
            .send_searcher_order(order)
            .await
            .expect("to not throw error"));
    }

    #[tokio::test]
    async fn test_send_partial_flash_order() {
        let (_handle, api) = setup_order_api();
        let order = PartialFlashOrder::default();
        assert!(api
            .send_partial_flash_order(order)
            .await
            .expect("to not throw error"));
    }

    #[tokio::test]
    async fn test_send_exact_flash_order() {
        let (_handle, api) = setup_order_api();
        let order = ExactFlashOrder::default();
        assert!(api
            .send_exact_flash_order(order)
            .await
            .expect("to not throw error"));
    }

    fn setup_order_api() -> (OrderApiTestHandle, OrderApi<MockOrderPoolHandle, TokioTaskExecutor>) {
        let (to_pool, pool_rx) = unbounded_channel();
        let pool_handle = MockOrderPoolHandle { sender: to_pool };
        let task_executor = TokioTaskExecutor::default();
        let api = OrderApi::new(pool_handle.clone(), task_executor);
        let handle = OrderApiTestHandle { from_api: pool_rx };
        (handle, api)
    }

    struct OrderApiTestHandle {
        from_api: UnboundedReceiver<OrderCommand>
    }

    #[derive(Clone)]
    struct MockOrderPoolHandle {
        sender: UnboundedSender<OrderCommand>
    }

    impl OrderPoolHandle for MockOrderPoolHandle {
        fn new_order(
            &self,
            origin: OrderOrigin,
            order: AllOrders
        ) -> impl Future<Output = bool> + Send {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let res = self
                .sender
                .send(OrderCommand::NewOrder(origin, order, tx))
                .is_ok();
            future::ready(true)
        }

        fn subscribe_orders(&self) -> Receiver<PoolManagerUpdate> {
            unimplemented!("Not needed for this test")
        }
    }
}
