use angstrom_rpc_types::metrics::MetricsEventEnvelope;
use jsonrpsee::proc_macros::rpc;

#[cfg_attr(not(feature = "client"), rpc(server, namespace = "metrics"))]
#[cfg_attr(feature = "client", rpc(server, client, namespace = "metrics"))]
#[async_trait::async_trait]
pub trait MetricsApi {
    #[subscription(
        name = "subscribeMetricEvents",
        unsubscribe = "unsubscribeMetricEvents",
        item = MetricsEventEnvelope
    )]
    async fn subscribe_metric_events(&self) -> jsonrpsee::core::SubscriptionResult;
}
