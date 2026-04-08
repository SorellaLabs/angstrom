use std::collections::HashSet;

use alloy_primitives::Address;
use angstrom_rpc_api::ConsensusApiServer;
use consensus::{
    AngstromValidator, ConsensusDataWithBlock, ConsensusHandle, ConsensusTimingConfig
};
use futures::StreamExt;
use jsonrpsee::{
    PendingSubscriptionSink, SubscriptionMessage,
    core::RpcResult,
    types::{ErrorCode, ErrorObjectOwned}
};
use reth_tasks::TaskExecutor;

pub struct ConsensusApi<Consensus> {
    consensus:    Consensus,
    task_executor: TaskExecutor
}

impl<Consensus> ConsensusApi<Consensus> {
    pub fn new(consensus: Consensus, task_executor: TaskExecutor) -> Self {
        Self { consensus, task_executor }
    }
}

#[async_trait::async_trait]
impl<Consensus> ConsensusApiServer for ConsensusApi<Consensus>
where
    Consensus: ConsensusHandle
{
    async fn get_current_leader(&self) -> RpcResult<ConsensusDataWithBlock<Address>> {
        Ok(self
            .consensus
            .get_current_leader()
            .await
            .map_err(|_| ErrorObjectOwned::from(ErrorCode::from(-1)))?)
    }

    async fn get_timing(&self) -> RpcResult<ConsensusDataWithBlock<ConsensusTimingConfig>> {
        Ok(self
            .consensus
            .timings()
            .await
            .map_err(|_| ErrorObjectOwned::from(ErrorCode::from(-1)))?)
    }

    async fn is_round_closed(&self) -> RpcResult<ConsensusDataWithBlock<bool>> {
        Ok(self
            .consensus
            .is_round_closed()
            .await
            .map_err(|_| ErrorObjectOwned::from(ErrorCode::from(-1)))?)
    }

    async fn fetch_consensus_state(
        &self
    ) -> RpcResult<ConsensusDataWithBlock<HashSet<AngstromValidator>>> {
        Ok(self
            .consensus
            .fetch_consensus_state()
            .await
            .map_err(|_| ErrorObjectOwned::from(ErrorCode::from(-1)))?)
    }

    async fn subscribe_empty_block_attestations(
        &self,
        pending: PendingSubscriptionSink
    ) -> jsonrpsee::core::SubscriptionResult {
        let sink = pending.accept().await?;
        let mut subscription = self.consensus.subscribe_empty_block_attestations();
        self.task_executor.spawn_task(async move {
            while let Some(result) = subscription.next().await {
                if sink.is_closed() {
                    break;
                }

                match SubscriptionMessage::new(sink.method_name(), sink.subscription_id(), &result)
                {
                    Ok(message) => {
                        if sink.send(message).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to serialize subscription message: {:?}", e);
                    }
                }
            }
        });

        Ok(())
    }
}
