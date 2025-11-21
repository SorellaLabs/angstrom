use std::collections::HashSet;

use angstrom_rpc_types::{GasEstimateFilter, GasEstimateUpdate};
use jsonrpsee::proc_macros::rpc;

#[cfg_attr(not(feature = "client"), rpc(server, namespace = "quoting"))]
#[cfg_attr(feature = "client", rpc(server, client, namespace = "quoting"))]
#[async_trait::async_trait]
pub trait QuotingApi {
    #[subscription(
        name = "subscribe_gas_estimates", 
        unsubscribe = "unsubscribe_gas_estimates",
        item = GasEstimateUpdate
    )]
    async fn subscribe_gas_estimates(
        &self,
        filters: HashSet<GasEstimateFilter>
    ) -> jsonrpsee::core::SubscriptionResult;
}
