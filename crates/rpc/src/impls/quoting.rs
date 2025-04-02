use std::collections::HashSet;

use jsonrpsee::PendingSubscriptionSink;
use reth_tasks::TaskSpawner;

use crate::{api::QuotingApiServer, types::GasEstimateFilter};

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
