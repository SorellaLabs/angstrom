use angstrom_rpc_types::metrics::BlockMetricsEventEnvelope;
use jsonrpsee::proc_macros::rpc;

#[cfg_attr(not(feature = "client"), rpc(server, namespace = "metrics"))]
#[cfg_attr(feature = "client", rpc(server, client, namespace = "metrics"))]
#[async_trait::async_trait]
pub trait MetricsApi {
    #[subscription(
        name = "subscribeBlockEvents",
        unsubscribe = "unsubscribeBlockEvents",
        item = BlockMetricsEventEnvelope
    )]
    async fn subscribe_block_events(&self) -> jsonrpsee::core::SubscriptionResult;
}
