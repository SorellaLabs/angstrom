use std::collections::HashSet;

use angstrom_rpc_api::QuotingApiServer;
use angstrom_rpc_types::GasEstimateFilter;
use jsonrpsee::PendingSubscriptionSink;
use reth_tasks::TaskSpawner;

pub struct QuotesApi<OrderPool, Spawner> {
    _pool:         OrderPool,
    _task_spawner: Spawner
}

#[async_trait::async_trait]
impl<OrderPool, Spawner> QuotingApiServer for QuotesApi<OrderPool, Spawner>
where
    OrderPool: Send + Sync + 'static,
    Spawner: TaskSpawner + 'static
{
    async fn subscribe_gas_estimates(
        &self,
        _pending: PendingSubscriptionSink,
        _filters: HashSet<GasEstimateFilter>
    ) -> jsonrpsee::core::SubscriptionResult {
        Ok(())
    }
}
