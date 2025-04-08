use std::collections::HashSet;

use alloy_primitives::{Address, B256, FixedBytes, U256};
use angstrom_types::{
    orders::{CancelOrderRequest, OrderLocation, OrderStatus},
    primitive::PoolId,
    sol_bindings::grouped_orders::AllOrders
};
use futures::StreamExt;
use jsonrpsee::{
    core::{RpcResult, Serialize},
    proc_macros::rpc
};
use serde::Deserialize;

use crate::types::{OrderSubscriptionFilter, OrderSubscriptionKind, PendingOrder};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct GasEstimateResponse {
    pub gas_units: u64,
    pub gas:       U256
}

#[cfg_attr(not(feature = "client"), rpc(server, namespace = "angstrom"))]
#[cfg_attr(feature = "client", rpc(server, client, namespace = "angstrom"))]
#[async_trait::async_trait]
pub trait OrderApi {
    /// Submit any type of order
    #[method(name = "sendOrder")]
    async fn send_order(&self, order: AllOrders) -> RpcResult<Result<FixedBytes<32>, String>>;

    #[method(name = "pendingOrder")]
    async fn pending_order(&self, from: Address) -> RpcResult<Vec<PendingOrder>>;

    #[method(name = "cancelOrder")]
    async fn cancel_order(&self, request: CancelOrderRequest) -> RpcResult<bool>;

    #[method(name = "estimateGas")]
    async fn estimate_gas(
        &self,
        is_book: bool,
        token_0: Address,
        token_1: Address
    ) -> RpcResult<Result<U256, String>>;

    #[method(name = "orderStatus")]
    async fn order_status(&self, order_hash: B256) -> RpcResult<OrderStatus>;

    #[method(name = "validNonce")]
    async fn valid_nonce(&self, user: Address) -> RpcResult<u64>;

    #[method(name = "ordersByPair")]
    async fn orders_by_pool_id(
        &self,
        pool_id: PoolId,
        location: OrderLocation
    ) -> RpcResult<Vec<AllOrders>>;

    #[subscription(
        name = "subscribeOrders",
        unsubscribe = "unsubscribeOrders",
        item = crate::types::subscriptions::OrderSubscriptionResult
    )]
    async fn subscribe_orders(
        &self,
        kind: HashSet<OrderSubscriptionKind>,
        filters: HashSet<OrderSubscriptionFilter>
    ) -> jsonrpsee::core::SubscriptionResult;

    // MULTI CALL
    #[method(name = "sendOrders")]
    async fn send_orders(
        &self,
        orders: Vec<AllOrders>
    ) -> RpcResult<Vec<Result<FixedBytes<32>, String>>> {
        futures::stream::iter(orders.into_iter())
            .map(|order| async { self.send_order(order).await })
            .buffered(3)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<RpcResult<Vec<_>>>()
    }

    #[method(name = "pendingOrders")]
    async fn pending_orders(&self, from: Vec<Address>) -> RpcResult<Vec<PendingOrder>> {
        Ok(futures::stream::iter(from.into_iter())
            .map(|order| async move { self.pending_order(order).await })
            .buffered(3)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<RpcResult<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect())
    }

    #[method(name = "cancelOrders")]
    async fn cancel_orders(&self, request: Vec<CancelOrderRequest>) -> RpcResult<Vec<bool>> {
        futures::stream::iter(request.into_iter())
            .map(|order| async { self.cancel_order(order).await })
            .buffered(3)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<RpcResult<Vec<_>>>()
    }

    #[method(name = "estimateGasOfOrders")]
    async fn estimate_gas_of_orders(
        &self,
        orders: Vec<(bool, Address, Address)>
    ) -> RpcResult<Vec<Result<U256, String>>> {
        futures::stream::iter(orders.into_iter())
            .map(|(is_book, token_0, token_1)| async move {
                self.estimate_gas(is_book, token_0, token_1).await
            })
            .buffered(3)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<RpcResult<Vec<_>>>()
    }

    #[method(name = "orderStatuses")]
    async fn status_of_orders(&self, order_hashes: Vec<B256>) -> RpcResult<Vec<OrderStatus>> {
        futures::stream::iter(order_hashes.into_iter())
            .map(|order| async move { self.order_status(order).await })
            .buffered(3)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<RpcResult<Vec<_>>>()
    }

    #[method(name = "ordersByPairs")]
    async fn orders_by_pool_ids(
        &self,
        pool_ids_with_location: Vec<(PoolId, OrderLocation)>
    ) -> RpcResult<Vec<AllOrders>> {
        Ok(futures::stream::iter(pool_ids_with_location.into_iter())
            .map(|(pair, location)| async move { self.orders_by_pool_id(pair, location).await })
            .buffered(3)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<RpcResult<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect())
    }
}
