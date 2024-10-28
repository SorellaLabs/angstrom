use std::{marker::PhantomData, sync::Arc};

use alloy::{
    network::{BlockResponse, HeaderResponse, Network},
    providers::Provider,
    rpc::types::Filter,
    transports::Transport
};
use alloy_primitives::Log;
use futures_util::{FutureExt, StreamExt};

use crate::cfmm::uniswap::{pool_manager::PoolManagerError, pool_providers::PoolManagerProvider};

pub struct ProviderAdapter<P, T, N>
where
    P: Provider<T, N> + Send + Sync,
    T: Transport + Clone + Send + Sync,
    N: Network + Send + Sync
{
    inner:    Arc<P>,
    _phantom: PhantomData<(T, N)>
}

impl<P, T, N> ProviderAdapter<P, T, N>
where
    P: Provider<T, N> + Send + Sync,
    T: Transport + Clone + Send + Sync,
    N: Network + Send + Sync
{
    pub fn new(inner: Arc<P>) -> Self {
        Self { inner, _phantom: PhantomData }
    }
}

impl<P, T, N> PoolManagerProvider for ProviderAdapter<P, T, N>
where
    P: Provider<T, N> + 'static + Send + Sync,
    T: Transport + Clone + Send + Sync,
    N: Network + Send + Sync
{
    fn subscribe_blocks(&self) -> futures::stream::BoxStream<Option<u64>> {
        let provider = self.inner.clone();
        async move { provider.subscribe_blocks().await.unwrap().into_stream() }
            .flatten_stream()
            .map(|b| Some(b.header().number()))
            .boxed()
    }

    async fn get_logs(&self, filter: &Filter) -> Result<Vec<Log>, PoolManagerError> {
        let alloy_logs = self
            .inner
            .get_logs(filter)
            .await
            .map_err(PoolManagerError::from)?;

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
