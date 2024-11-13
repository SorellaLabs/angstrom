use alloy_primitives::{Address, FixedBytes, B256};
use angstrom_types::{
    orders::{OrderLocation, OrderStatus},
    primitive::Signature,
    sol_bindings::grouped_orders::AllOrders
};
use jsonrpsee::{
    core::{RpcResult, Serialize},
    proc_macros::rpc
};
use serde::Deserialize;

use crate::types::OrderSubscriptionKind;

#[derive(Serialize, Deserialize, Debug)]
pub struct CancelOrderRequest {
    pub signature: Signature,
    pub hash:      B256
}

#[cfg_attr(not(feature = "client"), rpc(server, namespace = "angstrom"))]
#[cfg_attr(feature = "client", rpc(server, client, namespace = "angstrom"))]
#[async_trait::async_trait]
pub trait OrderApi {
    /// Submit any type of order
    #[method(name = "sendOrder")]
    async fn send_order(&self, order: AllOrders) -> RpcResult<bool>;

    #[method(name = "pendingOrders")]
    async fn pending_orders(&self, from: Address) -> RpcResult<Vec<AllOrders>>;

    #[method(name = "cancelOrder")]
    async fn cancel_order(&self, request: CancelOrderRequest) -> RpcResult<bool>;

    #[method(name = "estimateGas")]
    async fn estimate_gas(&self, order: AllOrders) -> RpcResult<u64>;

    #[method(name = "orderStatus")]
    async fn order_status(&self, order_hash: B256) -> RpcResult<Option<OrderStatus>>;

    #[method(name = "ordersByPair")]
    async fn orders_by_pair(
        &self,
        pair: FixedBytes<32>,
        location: OrderLocation
    ) -> RpcResult<Vec<AllOrders>>;

    #[subscription(
        name = "subscribeOrders",
        unsubscribe = "unsubscribeOrders",
        item = crate::types::subscriptions::OrderSubscriptionResult
    )]
    async fn subscribe_orders(
        &self,
        kind: OrderSubscriptionKind
    ) -> jsonrpsee::core::SubscriptionResult;
}
