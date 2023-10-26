use common::{AtomicConsensus, ConsensusState};
use consensus::ConsensusListener;
use jsonrpsee::{core::RpcResult, PendingSubscriptionSink};

use crate::{
    api::{ConsensusApiServer, ConsensusPubSubApiServer},
    types::ConsensusSubscriptionKind
};

pub struct ConsensusApi<Consensus> {
    consensus: Consensus,
    state:     AtomicConsensus
}

#[async_trait::async_trait]
impl<C> ConsensusApiServer for ConsensusApi<C>
where
    C: Send + Sync + 'static
{
    async fn consensus_state(&self) -> RpcResult<ConsensusState> {
        Ok(self.state.get_current_state())
    }
}

#[async_trait::async_trait]
impl<C> ConsensusPubSubApiServer for ConsensusApi<C>
where
    C: ConsensusListener + Send + Sync + 'static
{
    async fn subscribe(
        &self,
        pending: PendingSubscriptionSink,
        kind: ConsensusSubscriptionKind
    ) -> jsonrpsee::core::SubscriptionResult {
        todo!()
    }
}
