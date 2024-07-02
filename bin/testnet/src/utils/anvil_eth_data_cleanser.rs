use std::task::{Context, Poll};

use alloy_provider::{Provider, RootProvider};
use alloy_pubsub::{PubSubFrontend, Subscription};
use alloy_rpc_types_eth::Block;
use angstrom_eth::{
    handle::{EthCommand, EthHandle},
    manager::EthEvent
};
use futures::{Future, FutureExt};
use reth_tasks::TaskSpawner;
use tokio::sync::mpsc::{Receiver, Sender, UnboundedSender};
use tokio_stream::wrappers::ReceiverStream;

pub struct AnvilEthDataCleanser {
    /// our command receiver
    commander:          ReceiverStream<EthCommand>,
    /// people listening to events
    event_listeners:    Vec<UnboundedSender<EthEvent>>,
    provider:           RootProvider<PubSubFrontend>,
    block_subscription: Subscription<Block>
}

impl AnvilEthDataCleanser {
    pub async fn spawn<TP: TaskSpawner>(
        tp: TP,
        tx: Sender<EthCommand>,
        rx: Receiver<EthCommand>,
        provider: RootProvider<PubSubFrontend>
    ) -> eyre::Result<EthHandle> {
        let stream = ReceiverStream::new(rx);
        let block_subscription = provider.subscribe_blocks().await?;
        let this =
            Self { commander: stream, event_listeners: Vec::new(), provider, block_subscription };
        tp.spawn_critical("eth handle", this.boxed());

        let handle = EthHandle::new(tx);

        Ok(handle)
    }

    fn send_events(&mut self, event: EthEvent) {
        self.event_listeners
            .retain(|e| e.send(event.clone()).is_ok());
    }

    // fn on_command(&mut self, command: EthCommand) {
    //     match command {
    //         EthCommand::SubscribeEthNetworkEvents(tx) =>
    // self.event_listeners.push(tx)     }
    // }
    //
    // fn on_canon_update(&mut self, canonical_updates: CanonStateNotification) {
    //     match canonical_updates {
    //         CanonStateNotification::Reorg { old, new } => self.handle_reorg(old,
    // new),         CanonStateNotification::Commit { new } =>
    // self.handle_commit(new)     }
    // }
    //
    // fn handle_reorg(&mut self, old: Arc<Chain>, new: Arc<Chain>) {
    //     let mut eoas = Self::get_eoa(old.clone());
    //     eoas.extend(Self::get_eoa(new.clone()));
    //     // state changes
    //     let state_changes = EthEvent::EOAStateChanges(eoas);
    //     self.send_events(state_changes);
    //
    //     // get all reorged orders
    //     let old_filled: HashSet<_> =
    // Self::fetch_filled_orders(old.clone()).collect();     let new_filled:
    // HashSet<_> = Self::fetch_filled_orders(new.clone()).collect();
    //     let difference: Vec<_> =
    // old_filled.difference(&new_filled).copied().collect();
    //     let reorged_orders = EthEvent::ReorgedOrders(difference);
    //     self.send_events(reorged_orders);
    // }
    //
    // fn handle_commit(&mut self, new: Arc<Chain>) {
    //     let filled_orders =
    // Self::fetch_filled_orders(new.clone()).collect::<Vec<_>>();     let tip =
    // new.tip().number;     let filled_orders =
    // EthEvent::FilledOrders(filled_orders, tip);
    //     self.send_events(filled_orders);
    //
    //     let eoas = Self::get_eoa(new.clone());
    //     self.send_events(EthEvent::EOAStateChanges(eoas));
    // }
    //
    // /// TODO: check contract for state change. if there is change. fetch the
    // /// transaction on Angstrom and process call-data to pull order-hashes.
    // fn fetch_filled_orders(_chain: Arc<Chain>) -> impl Iterator<Item = B256> +
    // 'static {     vec![].into_iter()
    // }
    //
    // /// fetches all eoa addresses touched
    // fn get_eoa(chain: Arc<Chain>) -> Vec<Address> {
    //     // chain.state().state().state().keys().copied().collect()
    //     vec![]
    // }
}

impl Future for AnvilEthDataCleanser {
    type Output = ();

    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Pending
    }
}
