use std::sync::Arc;

use alloy::{consensus::BlockHeader, providers::Provider, rpc::types::Filter};
use alloy_primitives::Log;
use futures_util::{FutureExt, StreamExt};

use super::PoolMangerBlocks;
use crate::uniswap::{pool_manager::PoolManagerError, pool_providers::PoolManagerProvider};

#[derive(Clone)]
pub struct ProviderAdapter<P>
where
    P: Provider + Send + Sync
{
    inner: Arc<P>
}

impl<P> ProviderAdapter<P>
where
    P: Provider + Send + Sync
{
    pub fn new(inner: Arc<P>) -> Self {
        Self { inner }
    }
}

impl<P> PoolManagerProvider for ProviderAdapter<P>
where
    P: Provider + Send + Sync + Clone + 'static
{
    fn provider(&self) -> Arc<impl Provider> {
        self.inner.clone()
    }

    fn subscribe_blocks(self) -> futures::stream::BoxStream<'static, Option<PoolMangerBlocks>> {
        let provider = self.inner.clone();
        async move { provider.subscribe_blocks().await.unwrap().into_stream() }
            .flatten_stream()
            .map(|b| Some(PoolMangerBlocks::NewBlock(b.number())))
            .boxed()
    }

    fn get_logs(&self, filter: &Filter) -> Result<Vec<Log>, PoolManagerError> {
        let handle = tokio::runtime::Handle::try_current().expect("No tokio runtime found");
        let alloy_logs = tokio::task::block_in_place(|| {
            handle.block_on(async {
                self.inner
                    .get_logs(filter)
                    .await
                    .map_err(PoolManagerError::from)
            })
        })?;

        let reth_logs = alloy_logs
            .iter()
            .map(|alloy_log| Log {
                address: alloy_log.address(),
                data:    alloy_log.data().clone()
            })
            .collect();

        Ok(reth_logs)
    }
}
