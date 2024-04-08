use angstrom_utils::ConsensusState;
use jsonrpsee::{core::RpcResult, PendingSubscriptionSink};

use crate::{api::ConsensusApiServer, types::ConsensusSubscriptionKind};

pub struct ConsensusApi<C> {
    pub consensus: C
}

#[async_trait::async_trait]
impl<C> ConsensusApiServer for ConsensusApi<C>
where
    C: Send + Sync + 'static
{
    async fn consensus_state(&self) -> RpcResult<ConsensusState> {
        todo!()
    }

    async fn subscribe_consensus_state(
        &self,
        _pending: PendingSubscriptionSink,
        _kind: ConsensusSubscriptionKind
    ) -> jsonrpsee::core::SubscriptionResult {
        todo!()
    }
}
