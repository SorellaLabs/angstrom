mod leader_selection;
mod manager;

use angstrom_types::consensus::ConsensusRoundOrderHashes;
pub use manager::*;
pub mod rounds;
use std::{collections::HashSet, pin::Pin, time::Duration};

use alloy::primitives::{Address, Bytes};
use futures::{Stream, StreamExt};
pub use leader_selection::AngstromValidator;
use serde::{Deserialize, Serialize};
use tokio::sync::{
    mpsc::{self, channel},
    oneshot
};
use tokio_stream::wrappers::ReceiverStream;

#[derive(Debug, Clone, Copy, clap::Args, Serialize, Deserialize)]
pub struct ConsensusTimingConfig {
    #[clap(long, default_value_t = 8_000)]
    pub min_wait_duration_ms: u64,
    #[clap(long, default_value_t = 9_000)]
    pub max_wait_duration_ms: u64
}

impl Default for ConsensusTimingConfig {
    fn default() -> Self {
        Self { min_wait_duration_ms: 8_000, max_wait_duration_ms: 9_000 }
    }
}

impl ConsensusTimingConfig {
    pub fn is_valid(&self) -> bool {
        self.min_wait_duration_ms < self.max_wait_duration_ms
    }

    pub const fn min_wait_time_ms(&self) -> Duration {
        Duration::from_millis(self.min_wait_duration_ms)
    }

    pub const fn max_wait_time_ms(&self) -> Duration {
        Duration::from_millis(self.max_wait_duration_ms)
    }

    pub fn default_duration(&self) -> Duration {
        Duration::from_secs_f64(
            (self.max_wait_time_ms() + self.min_wait_time_ms()).as_secs_f64() / 2.0
        )
    }
}

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

    fn is_round_closed(
        &self
    ) -> impl Future<Output = eyre::Result<ConsensusDataWithBlock<bool>>> + Send;

    fn timings(
        &self
    ) -> impl Future<Output = eyre::Result<ConsensusDataWithBlock<ConsensusTimingConfig>>> + Send;
}

#[derive(Clone)]
pub struct ConsensusHandler(pub tokio::sync::mpsc::UnboundedSender<ConsensusRequest>);

impl ConsensusHandle for ConsensusHandler {
    async fn timings(&self) -> eyre::Result<ConsensusDataWithBlock<ConsensusTimingConfig>> {
        let (tx, rx) = oneshot::channel();
        self.0.send(ConsensusRequest::Timing(tx))?;

        rx.await.map_err(Into::into)
    }

    async fn is_round_closed(&self) -> eyre::Result<ConsensusDataWithBlock<bool>> {
        let (tx, rx) = oneshot::channel();
        self.0.send(ConsensusRequest::IsRoundClosed(tx))?;

        rx.await.map_err(Into::into)
    }

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
    IsRoundClosed(oneshot::Sender<ConsensusDataWithBlock<bool>>),
    Timing(oneshot::Sender<ConsensusDataWithBlock<ConsensusTimingConfig>>),
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
