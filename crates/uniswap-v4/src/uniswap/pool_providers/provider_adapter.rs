use std::{marker::PhantomData, sync::Arc};

use alloy::{consensus::BlockHeader, providers::Provider, rpc::types::Filter};
use alloy_primitives::Log;
use angstrom_types::provider::NetworkProvider;
use futures_util::{FutureExt, StreamExt};

use super::PoolMangerBlocks;
use crate::uniswap::{pool_manager::PoolManagerError, pool_providers::PoolManagerProvider};

#[derive(Clone)]
pub struct ProviderAdapter<P, N>
where
    P: Provider<N::Network> + Send + Sync,
    N: NetworkProvider
{
    inner:    Arc<P>,
    _phantom: PhantomData<N>
}

impl<P, N> ProviderAdapter<P, N>
where
    P: Provider<N::Network> + Send + Sync + Clone + 'static,
    N: NetworkProvider
{
    pub fn new(inner: Arc<P>) -> Self {
        Self { inner, _phantom: PhantomData }
    }
}

impl<P, N> PoolManagerProvider<P, N> for ProviderAdapter<P, N>
where
    P: Provider<N::Network> + Send + Sync + Clone + 'static,
    N: NetworkProvider
{
    fn provider(&self) -> Arc<P> {
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
