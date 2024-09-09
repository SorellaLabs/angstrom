use std::{
    collections::HashSet,
    sync::Arc,
    task::{Context, Poll}
};

use alloy_primitives::{Address, B256};
use futures::Future;
use futures_util::{FutureExt, StreamExt};
use reth_provider::{CanonStateNotification, CanonStateNotifications, Chain, StateProviderFactory};
use reth_tasks::TaskSpawner;
use tokio::sync::mpsc::{Receiver, Sender, UnboundedSender};
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};

use crate::handle::{EthCommand, EthHandle};

/// Listens for CanonStateNotifications and sends the appropriate updates to be
/// executed by the order pool
#[allow(dead_code)]
pub struct EthDataCleanser<DB> {
    /// our command receiver
    commander:       ReceiverStream<EthCommand>,
    /// people listening to events
    event_listeners: Vec<UnboundedSender<EthEvent>>,

    /// Notifications for Canonical Block updates
    canonical_updates: BroadcastStream<CanonStateNotification>,
    /// used to fetch data from db
    #[allow(dead_code)]
    db:                DB
}

impl<DB> EthDataCleanser<DB>
where
    DB: StateProviderFactory + Send + Sync + Unpin + 'static
{
    pub fn spawn<TP: TaskSpawner>(
        canonical_updates: CanonStateNotifications,
        db: DB,
        tp: TP,
        tx: Sender<EthCommand>,
        rx: Receiver<EthCommand>
    ) -> anyhow::Result<EthHandle> {
        let stream = ReceiverStream::new(rx);

        let this = Self {
            canonical_updates: BroadcastStream::new(canonical_updates),
            commander: stream,
            event_listeners: Vec::new(),
            db
        };
        tp.spawn_critical("eth handle", this.boxed());

        let handle = EthHandle::new(tx);

        Ok(handle)
    }

    fn send_events(&mut self, event: EthEvent) {
        self.event_listeners
            .retain(|e| e.send(event.clone()).is_ok());
    }

    fn on_command(&mut self, command: EthCommand) {
        match command {
            EthCommand::SubscribeEthNetworkEvents(tx) => self.event_listeners.push(tx)
        }
    }

    fn on_canon_update(&mut self, canonical_updates: CanonStateNotification) {
        match canonical_updates {
            CanonStateNotification::Reorg { old, new } => self.handle_reorg(old, new),
            CanonStateNotification::Commit { new } => self.handle_commit(new)
        }
    }

    fn handle_reorg(&mut self, old: Arc<Chain>, new: Arc<Chain>) {
        // let mut eoas = Self::get_eoa(old.clone());
        // eoas.extend(Self::get_eoa(new.clone()));
        // // state changes
        // let state_changes = EthEvent::EOAStateChanges(eoas);
        // self.send_events(state_changes);

        // get all reorged orders
        let old_filled: HashSet<_> = Self::fetch_filled_orders(old.clone()).collect();
        let new_filled: HashSet<_> = Self::fetch_filled_orders(new.clone()).collect();
        let difference: Vec<_> = old_filled.difference(&new_filled).copied().collect();
        let reorged_orders = EthEvent::ReorgedOrders(difference);
        self.send_events(reorged_orders);
    }

    fn handle_commit(&mut self, new: Arc<Chain>) {
        // let filled_orders =
        // Self::fetch_filled_orders(new.clone()).collect::<Vec<_>>();
        // let tip = new.tip().number;
        // let filled_orders = EthEvent::FilledOrders(filled_orders, tip);
        // self.send_events(filled_orders);
        //
        // let eoas = Self::get_eoa(new.clone());
        // self.send_events(EthEvent::EOAStateChanges(eoas));
    }

    /// TODO: check contract for state change. if there is change. fetch the
    /// transaction on Angstrom and process call-data to pull order-hashes.
    fn fetch_filled_orders(_chain: Arc<Chain>) -> impl Iterator<Item = B256> + 'static {
        vec![].into_iter()
    }

    /// fetches all eoa addresses touched
    fn get_eoa(_chain: Arc<Chain>) -> Vec<Address> {
        // chain.state().state().state().keys().copied().collect()
        vec![]
    }
}

impl<DB> Future for EthDataCleanser<DB>
where
    DB: StateProviderFactory + Send + Sync + Unpin + 'static
{
    type Output = ();

    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // poll all canonical updates
        while let Poll::Ready(is_some) = self.canonical_updates.poll_next_unpin(cx).map(|res| {
            res.transpose()
                .ok()
                .flatten()
                .map(|update| self.on_canon_update(update))
                .is_some()
        }) {
            if !is_some {
                return Poll::Ready(())
            }
        }

        while let Poll::Ready(Some(command)) = self.commander.poll_next_unpin(cx) {
            self.on_command(command)
        }

        Poll::Pending
    }
}

#[derive(Debug, Clone)]
pub enum EthEvent {
    //TODO: add shit here
    NewBlock(u64),
    NewBlockTransitions {
        block_number:      u64,
        filled_orders:     Vec<B256>,
        address_changeset: Vec<Address>
    },
    ReorgedOrders(Vec<B256>),
    FinalizedBlock(u64)
}
