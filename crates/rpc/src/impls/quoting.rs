use std::collections::HashSet;

use alloy_primitives::{Address, U256};
use jsonrpsee::{core::RpcResult, PendingSubscriptionSink, SubscriptionMessage};

use crate::{
    api::QuotingApiServer,
    types::{QuotingSubscriptionKind, QuotingSubscriptionParam}
};

pub struct QuotesApi<OrderPool> {
    pub pool: OrderPool
}

#[async_trait::async_trait]
impl<OrderPool> QuotingApiServer for QuotesApi<OrderPool>
where
    OrderPool: Send + Sync + 'static
{
    async fn subscribe_gas_estimates(
        &self,
        pending: PendingSubscriptionSink,
        filters: HashSet<GasEstimateFilter>
    ) -> jsonrpsee::core::SubscriptionResult {
        Ok(())
    }
}
