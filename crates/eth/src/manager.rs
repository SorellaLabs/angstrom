use std::task::{Context, Poll};

use alloy_primitives::{Address, B256};
use futures::Future;
use futures_util::FutureExt;
use reth_provider::{CanonStateNotification, CanonStateNotifications, StateProviderFactory};
use reth_tasks::TaskSpawner;
use tokio::sync::mpsc::{channel, UnboundedSender};
use tokio_stream::wrappers::ReceiverStream;

use crate::handle::{EthCommand, EthHandle};

/// Listens for CanonStateNotifications and sends the appropriate updatdes to be
/// executed by the order pool
#[allow(dead_code)]
pub struct EthDataCleanser<DB> {
    /// our command receiver
    commander:       ReceiverStream<EthCommand>,
    /// people listening to events
    event_listeners: Vec<UnboundedSender<EthEvent>>,

    /// Notifications for Canonical Block updates
    canonical_updates: CanonStateNotifications,
    /// used to fetch data from db
    db:                DB
}

impl<DB> EthDataCleanser<DB>
where
    DB: StateProviderFactory + Send + Sync + Unpin + 'static
{
    pub fn new<TP: TaskSpawner>(
        canonical_updates: CanonStateNotifications,
        db: DB,
        tp: TP
    ) -> anyhow::Result<EthHandle> {
        let (tx, rx) = channel(10);
        let stream = ReceiverStream::new(rx);

        let this = Self { canonical_updates, commander: stream, event_listeners: Vec::new(), db };
        tp.spawn_critical("eth handle", this.boxed());

        let handle = EthHandle::new(tx);

        Ok(handle)
    }

    #[allow(dead_code)]
    fn on_canon_update(&mut self, canonical_updates: CanonStateNotification) {
        match canonical_updates {
            CanonStateNotification::Reorg { old: _old, new: _ } => {}
            CanonStateNotification::Commit { new: _new } => {}
        }
    }
}

impl<DB> Future for EthDataCleanser<DB>
where
    DB: StateProviderFactory + Send + Sync + Unpin + 'static
{
    type Output = ();

    fn poll(self: std::pin::Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        todo!()
    }
}
pub enum EthEvent {
    //TODO: add shit here
    NewBlock,
    FilledOrders(Vec<B256>, u64),
    EOAStateChanges(Vec<Address>),
    ReorgedOrders(Vec<B256>),
    FinalizedBlock(u64)
}
