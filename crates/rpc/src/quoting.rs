use std::collections::HashSet;

use angstrom_rpc_api::QuotingApiServer;
use angstrom_rpc_types::GasEstimateFilter;
use jsonrpsee::PendingSubscriptionSink;
use reth_tasks::TaskExecutor;

pub struct QuotesApi<OrderPool> {
    _pool:          OrderPool,
    _task_executor: TaskExecutor
}

#[async_trait::async_trait]
impl<OrderPool> QuotingApiServer for QuotesApi<OrderPool>
where
    OrderPool: Send + Sync + 'static
{
    async fn subscribe_gas_estimates(
        &self,
        _pending: PendingSubscriptionSink,
        _filters: HashSet<GasEstimateFilter>
    ) -> jsonrpsee::core::SubscriptionResult {
        Ok(())
    }
}
