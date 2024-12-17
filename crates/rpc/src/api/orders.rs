use std::collections::HashSet;

use alloy_primitives::{keccak256, Address, FixedBytes, PrimitiveSignature, B256, U256};
use alloy_sol_types::SolValue;
use angstrom_types::{
    orders::{OrderLocation, OrderStatus},
    sol_bindings::grouped_orders::AllOrders
};
use futures::StreamExt;
use jsonrpsee::{
    core::{RpcResult, Serialize},
    proc_macros::rpc
};
use serde::Deserialize;

use crate::types::{OrderSubscriptionFilter, OrderSubscriptionKind};

#[derive(Serialize, Deserialize, Debug)]
pub struct CancelOrderRequest {
    pub signature:    PrimitiveSignature,
    // if there's no salt to make this a unique signing hash. One can just
    // copy the signature of the order and id and it will verify
    pub user_address: Address,
    pub order_id:     B256
}

impl CancelOrderRequest {
    pub fn signing_payload(&self) -> FixedBytes<32> {
        keccak256((self.user_address, self.order_id).abi_encode())
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
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
    async fn send_order(&self, order: AllOrders) -> RpcResult<bool>;

    #[method(name = "pendingOrder")]
    async fn pending_order(&self, from: Address) -> RpcResult<Vec<AllOrders>>;

    #[method(name = "cancelOrder")]
    async fn cancel_order(&self, request: CancelOrderRequest) -> RpcResult<bool>;

    #[method(name = "estimateGas")]
    async fn estimate_gas(&self, order: AllOrders) -> RpcResult<GasEstimateResponse>;

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
        kind: HashSet<OrderSubscriptionKind>,
        filters: HashSet<OrderSubscriptionFilter>
    ) -> jsonrpsee::core::SubscriptionResult;

    // MULTI CALL
    #[method(name = "sendOrders")]
    async fn send_orders(&self, orders: Vec<AllOrders>) -> RpcResult<Vec<bool>> {
        futures::stream::iter(orders.into_iter())
            .map(|order| async { self.send_order(order).await })
            .buffered(3)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<RpcResult<Vec<_>>>()
    }

    #[method(name = "pendingOrders")]
    async fn pending_orders(&self, from: Vec<Address>) -> RpcResult<Vec<AllOrders>> {
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
        orders: Vec<AllOrders>
    ) -> RpcResult<Vec<GasEstimateResponse>> {
        futures::stream::iter(orders.into_iter())
            .map(|order| async { self.estimate_gas(order).await })
            .buffered(3)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<RpcResult<Vec<_>>>()
    }

    #[method(name = "orderStatuses")]
    async fn status_of_orders(
        &self,
        order_hashes: Vec<B256>
    ) -> RpcResult<Vec<Option<OrderStatus>>> {
        futures::stream::iter(order_hashes.into_iter())
            .map(|order| async move { self.order_status(order).await })
            .buffered(3)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<RpcResult<Vec<_>>>()
    }

    #[method(name = "ordersByPairs")]
    async fn orders_by_pairs(
        &self,
        pair_with_location: Vec<(FixedBytes<32>, OrderLocation)>
    ) -> RpcResult<Vec<AllOrders>> {
        Ok(futures::stream::iter(pair_with_location.into_iter())
            .map(|(pair, location)| async move { self.orders_by_pair(pair, location).await })
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
