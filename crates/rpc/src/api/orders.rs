use alloy_primitives::Bytes;
use jsonrpsee::{core::RpcResult, proc_macros::rpc};

use crate::types::subscriptions::OrderSubscriptionKind;

#[cfg_attr(not(feature = "client"), rpc(server, namespace = "angstrom_order"))]
#[cfg_attr(feature = "client", rpc(server, client, namespace = "angstrom_order"))]
#[async_trait::async_trait]
pub trait OrderApi {
    /// Users send the rlp encoded signature and order bytes
    #[method(name = "submit_limit_order")]
    async fn submit_limit_order(&self, order: Bytes) -> RpcResult<bool>;

    #[method(name = "submit_searcher_order")]
    async fn submit_searcher_order(&self, order: Bytes) -> RpcResult<bool>;

    #[method(name = "submit_composable_limit_order")]
    async fn submit_composable_limit_order(&self, order: Bytes) -> RpcResult<bool>;

    #[subscription(
        name = "orders_subscription", 
        unsubscribe = "unsubscribe_orders",
        item = crate::types::subscriptions::OrderSubscriptionResult
    )]
    async fn subscribe_orders(
        &self,
        kind: OrderSubscriptionKind
    ) -> jsonrpsee::core::SubscriptionResult;
}
