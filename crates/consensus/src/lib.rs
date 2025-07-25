mod leader_selection;
mod manager;

use angstrom_types::consensus::ConsensusRoundOrderHashes;
pub use manager::*;
pub mod rounds;
use std::{collections::HashSet, pin::Pin};

use alloy::primitives::{Address, Bytes};
use futures::{Stream, StreamExt};
pub use leader_selection::AngstromValidator;
use serde::{Deserialize, Serialize};
use tokio::sync::{
    mpsc::{self, channel},
    oneshot
};
use tokio_stream::wrappers::ReceiverStream;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ConsensusDataWithBlock<T> {
    pub data:  T,
    pub block: u64
}

pub trait ConsensusHandle: Send + Sync + Clone + Unpin + 'static {
    fn subscribe_empty_block_attestations(
        &self
    ) -> Pin<Box<dyn Stream<Item = ConsensusDataWithBlock<Bytes>> + Send>>;

    fn get_current_leader(
        &self
    ) -> impl Future<Output = eyre::Result<ConsensusDataWithBlock<Address>>> + Send;

    fn fetch_consensus_state(
        &self
    ) -> impl Future<Output = eyre::Result<ConsensusDataWithBlock<HashSet<AngstromValidator>>>> + Send;
}

#[derive(Clone)]
pub struct ConsensusHandler(pub tokio::sync::mpsc::UnboundedSender<ConsensusRequest>);

impl ConsensusHandle for ConsensusHandler {
    async fn get_current_leader(&self) -> eyre::Result<ConsensusDataWithBlock<Address>> {
        let (tx, rx) = oneshot::channel();
        self.0.send(ConsensusRequest::CurrentLeader(tx))?;

        rx.await.map_err(Into::into)
    }

    async fn fetch_consensus_state(
        &self
    ) -> eyre::Result<ConsensusDataWithBlock<HashSet<AngstromValidator>>> {
        let (tx, rx) = oneshot::channel();
        self.0.send(ConsensusRequest::CurrentConsensusState(tx))?;

        rx.await.map_err(Into::into)
    }

    fn subscribe_empty_block_attestations(
        &self
    ) -> Pin<Box<dyn Stream<Item = ConsensusDataWithBlock<Bytes>> + Send>> {
        let (tx, rx) = channel(5);
        let _ = self.0.send(ConsensusRequest::SubscribeAttestations(tx));

        Box::pin(ReceiverStream::new(rx).then(async |sub_req| match sub_req {
            ConsensusSubscriptionData::Attestations(attestations) => attestations,
            _ => unreachable!()
        }))
    }
}

impl ConsensusHandler {
    pub fn subscribe_consensus_round_event(
        &self
    ) -> Pin<Box<dyn Stream<Item = ConsensusRoundOrderHashes> + Send>> {
        let (tx, rx) = channel(5);
        let _ = self.0.send(ConsensusRequest::SubscribeRoundEventOrders(tx));

        Box::pin(ReceiverStream::new(rx).then(async |sub_req| match sub_req {
            ConsensusSubscriptionData::RoundEventOrders(order_hashes) => order_hashes,
            _ => unreachable!()
        }))
    }
}

pub enum ConsensusRequest {
    CurrentLeader(oneshot::Sender<ConsensusDataWithBlock<Address>>),
    CurrentConsensusState(oneshot::Sender<ConsensusDataWithBlock<HashSet<AngstromValidator>>>),
    SubscribeAttestations(mpsc::Sender<ConsensusSubscriptionData>),
    SubscribeRoundEventOrders(mpsc::Sender<ConsensusSubscriptionData>)
}

impl ConsensusRequest {
    pub fn subscription_kind(&self) -> Option<ConsensusSubscriptionRequestKind> {
        match self {
            ConsensusRequest::SubscribeAttestations(_) => {
                Some(ConsensusSubscriptionRequestKind::Attestations)
            }
            ConsensusRequest::SubscribeRoundEventOrders(_) => {
                Some(ConsensusSubscriptionRequestKind::RoundEventOrders)
            }
            _ => None
        }
    }
}

pub enum ConsensusSubscriptionData {
    Attestations(ConsensusDataWithBlock<Bytes>),
    RoundEventOrders(ConsensusRoundOrderHashes)
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConsensusSubscriptionRequestKind {
    Attestations,
    RoundEventOrders
}
