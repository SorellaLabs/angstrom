use alloy_primitives::{Address, FixedBytes, B256};
use angstrom_types::{
    orders::{OrderLocation, OrderOrigin, OrderStatus},
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
    api::{CancelOrderRequest, OrderApiServer},
    types::{OrderSubscriptionKind, OrderSubscriptionResult},
    OrderApiError::SignatureRecoveryError
};

pub struct OrderApi<OrderPool, Spawner> {
    pool:         OrderPool,
    task_spawner: Spawner
}

impl<OrderPool, Spawner> OrderApi<OrderPool, Spawner> {
    pub fn new(pool: OrderPool, task_spawner: Spawner) -> Self {
        Self { pool, task_spawner }
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

    async fn pending_orders(&self, from: Address) -> RpcResult<Vec<AllOrders>> {
        Ok(self.pool.pending_orders(from).await)
    }

    async fn cancel_order(&self, request: CancelOrderRequest) -> RpcResult<bool> {
        let sender = request
            .signature
            .recover_signer_full_public_key(request.hash)
            .map(|s| Address::from_raw_public_key(&*s))
            .map_err(|_| SignatureRecoveryError)?;

        Ok(self.pool.cancel_order(sender, request.hash).await)
    }

    async fn order_status(&self, order_hash: B256) -> RpcResult<Option<OrderStatus>> {
        Ok(self.pool.fetch_order_status(order_hash).await)
    }

    async fn orders_by_pair(
        &self,
        pair: FixedBytes<32>,
        location: OrderLocation
    ) -> RpcResult<Vec<AllOrders>> {
        Ok(self.pool.fetch_orders_from_pool(pair, location).await)
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
                    break
                }

                let msg = Self::return_order(&kind, order);
                if let Some(result) = msg {
                    match SubscriptionMessage::from_json(&result) {
                        Ok(message) => {
                            if sink.send(message).await.is_err() {
                                break
                            }
                        }
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

#[derive(Debug, thiserror::Error)]
pub enum OrderApiError {
    #[error("invalid transaction signature")]
    InvalidSignature,
    #[error("failed to recover signer from signature")]
    SignatureRecoveryError
}

impl From<OrderApiError> for jsonrpsee::types::ErrorObjectOwned {
    fn from(error: OrderApiError) -> Self {
        match error {
            OrderApiError::InvalidSignature => invalid_params_rpc_err(error.to_string()),
            OrderApiError::SignatureRecoveryError => invalid_params_rpc_err(error.to_string())
        }
    }
}

pub fn invalid_params_rpc_err(msg: impl Into<String>) -> jsonrpsee::types::ErrorObjectOwned {
    rpc_err(jsonrpsee::types::error::INVALID_PARAMS_CODE, msg, None)
}

pub fn rpc_err(
    code: i32,
    msg: impl Into<String>,
    data: Option<&[u8]>
) -> jsonrpsee::types::error::ErrorObjectOwned {
    jsonrpsee::types::error::ErrorObject::owned(
        code,
        msg.into(),
        data.map(|data| {
            jsonrpsee::core::to_json_raw_value(&alloy_primitives::hex::encode_prefixed(data))
                .expect("serializing String can't fail")
        })
    )
}

impl<OrderPool, Spawner> OrderApi<OrderPool, Spawner>
where
    OrderPool: OrderPoolHandle,
    Spawner: 'static + TaskSpawner
{
    fn return_order(
        kind: &OrderSubscriptionKind,
        order: PoolManagerUpdate
    ) -> Option<OrderSubscriptionResult> {
        match (&kind, order) {
            (OrderSubscriptionKind::NewOrders, PoolManagerUpdate::NewOrder(order_update)) => {
                Some(OrderSubscriptionResult::NewOrder(order_update))
            }
            (
                OrderSubscriptionKind::FilledOrders,
                PoolManagerUpdate::FilledOrder((block_number, filled_order))
            ) => Some(OrderSubscriptionResult::FilledOrder((block_number, filled_order))),
            (
                OrderSubscriptionKind::UnfilleOrders,
                PoolManagerUpdate::UnfilledOrders(unfilled_order)
            ) => Some(OrderSubscriptionResult::UnfilledOrder(unfilled_order)),
            (
                OrderSubscriptionKind::CancelledOrders,
                PoolManagerUpdate::CancelledOrder(order_hash)
            ) => Some(OrderSubscriptionResult::CancelledOrder(order_hash)),
            (OrderSubscriptionKind::NewOrders, PoolManagerUpdate::FilledOrder(_)) => None,
            (OrderSubscriptionKind::NewOrders, PoolManagerUpdate::UnfilledOrders(_)) => None,
            (OrderSubscriptionKind::FilledOrders, PoolManagerUpdate::NewOrder(_)) => None,
            (OrderSubscriptionKind::FilledOrders, PoolManagerUpdate::UnfilledOrders(_)) => None,
            (OrderSubscriptionKind::UnfilleOrders, PoolManagerUpdate::NewOrder(_)) => None,
            (OrderSubscriptionKind::UnfilleOrders, PoolManagerUpdate::FilledOrder(_)) => None,
            (OrderSubscriptionKind::NewOrders, PoolManagerUpdate::CancelledOrder(_)) => None,
            (OrderSubscriptionKind::FilledOrders, PoolManagerUpdate::CancelledOrder(_)) => None,
            (OrderSubscriptionKind::UnfilleOrders, PoolManagerUpdate::CancelledOrder(_)) => None,
            (OrderSubscriptionKind::CancelledOrders, PoolManagerUpdate::NewOrder(_)) => None,
            (OrderSubscriptionKind::CancelledOrders, PoolManagerUpdate::FilledOrder(_)) => None,
            (OrderSubscriptionKind::CancelledOrders, PoolManagerUpdate::UnfilledOrders(_)) => None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{future, future::Future};

    use alloy_primitives::{Address, B256};
    use angstrom_network::pool_manager::OrderCommand;
    use angstrom_types::sol_bindings::rpc_orders::{
        ExactFlashOrder, ExactStandingOrder, PartialFlashOrder, PartialStandingOrder,
        TopOfBlockOrder
    };
    use futures::FutureExt;
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
        let handle = OrderApiTestHandle { _from_api: pool_rx };
        (handle, api)
    }

    struct OrderApiTestHandle {
        _from_api: UnboundedReceiver<OrderCommand>
    }

    #[derive(Clone)]
    struct MockOrderPoolHandle {
        sender: UnboundedSender<OrderCommand>
    }

    impl OrderPoolHandle for MockOrderPoolHandle {
        fn fetch_orders_from_pool(
            &self,
            _: FixedBytes<32>,
            _: OrderLocation
        ) -> impl Future<Output = Vec<AllOrders>> + Send {
            future::ready(vec![])
        }

        fn new_order(
            &self,
            origin: OrderOrigin,
            order: AllOrders
        ) -> impl Future<Output = bool> + Send {
            let (tx, _) = tokio::sync::oneshot::channel();
            let _ = self
                .sender
                .send(OrderCommand::NewOrder(origin, order, tx))
                .is_ok();
            future::ready(true)
        }

        fn subscribe_orders(&self) -> Receiver<PoolManagerUpdate> {
            unimplemented!("Not needed for this test")
        }

        fn cancel_order(
            &self,
            from: Address,
            order_hash: B256
        ) -> impl Future<Output = bool> + Send {
            let (tx, _) = tokio::sync::oneshot::channel();
            let _ = self
                .sender
                .send(OrderCommand::CancelOrder(from, order_hash, tx))
                .is_ok();
            future::ready(true)
        }

        fn pending_orders(&self, address: Address) -> impl Future<Output = Vec<AllOrders>> + Send {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let _ = self
                .sender
                .send(OrderCommand::PendingOrders(address, tx))
                .is_ok();
            rx.map(|res| res.unwrap_or_default())
        }

        fn fetch_order_status(&self, _: B256) -> impl Future<Output = Option<OrderStatus>> + Send {
            future::ready(None)
        }
    }
}
