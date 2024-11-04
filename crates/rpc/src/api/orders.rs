use alloy_primitives::{Address, B256};
use angstrom_types::{
    primitive::Signature,
    sol_bindings::{
        grouped_orders::AllOrders,
        rpc_orders::{
            ExactFlashOrder, ExactStandingOrder, PartialFlashOrder, PartialStandingOrder,
            TopOfBlockOrder
        }
    }
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
    /// Users send the rlp encoded signature and order bytes
    #[method(name = "sendPartialStandingOrder")]
    async fn send_partial_standing_order(&self, order: PartialStandingOrder) -> RpcResult<bool>;

    #[method(name = "sendExactStandingOrder")]
    async fn send_exact_standing_order(&self, order: ExactStandingOrder) -> RpcResult<bool>;

    #[method(name = "sendSearcherOrder")]
    async fn send_searcher_order(&self, order: TopOfBlockOrder) -> RpcResult<bool>;

    #[method(name = "sendPartialFlashOrder")]
    async fn send_partial_flash_order(&self, order: PartialFlashOrder) -> RpcResult<bool>;

    #[method(name = "sendExactFlashOrder")]
    async fn send_exact_flash_order(&self, order: ExactFlashOrder) -> RpcResult<bool>;

    #[method(name = "pendingOrders")]
    async fn pending_orders(&self, from: Address) -> RpcResult<Vec<AllOrders>>;

    #[method(name = "cancelOrder")]
    async fn cancel_order(&self, request: CancelOrderRequest) -> RpcResult<bool>;

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
