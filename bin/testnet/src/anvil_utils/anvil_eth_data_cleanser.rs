use std::task::{Context, Poll};

use alloy_primitives::Address;
use alloy_rpc_types_eth::Block;
use alloy_sol_types::SolType;
use angstrom_eth::{
    handle::{EthCommand, EthHandle},
    manager::EthEvent
};
use futures::{Future, Stream, StreamExt};
use reth_tasks::TaskSpawner;
use sol_bindings::sol::ContractBundle;
use tokio::sync::mpsc::{Receiver, Sender, UnboundedSender};
use tokio_stream::wrappers::ReceiverStream;
use tracing::{Instrument, Span};

pub struct AnvilEthDataCleanser<S: Stream<Item = Block>> {
    angstrom_contract:           Address,
    /// our command receiver
    commander:                   ReceiverStream<EthCommand>,
    /// people listening to events
    event_listeners:             Vec<UnboundedSender<EthEvent>>,
    block_subscription:          S,
    block_finalization_lookback: u64
}

impl<S: Stream<Item = Block> + Unpin + Send + 'static> AnvilEthDataCleanser<S> {
    pub async fn spawn<TP: TaskSpawner>(
        tp: TP,
        angstrom_contract: Address,
        tx: Sender<EthCommand>,
        rx: Receiver<EthCommand>,
        block_subscription: S,
        block_finalization_lookback: u64,
        span: Span
    ) -> eyre::Result<EthHandle> {
        let stream = ReceiverStream::new(rx);
        let this = Self {
            commander: stream,
            event_listeners: Vec::new(),
            block_subscription,
            angstrom_contract,
            block_finalization_lookback
        };
        tp.spawn_critical("eth handle", Box::pin(this.instrument(span)));

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

    fn on_new_block(&mut self, block: Block) {
        let bn = block
            .header
            .number
            .expect("block from anvil with no number");

        self.send_events(EthEvent::NewBlock(bn));
        // no underflows today
        if bn > self.block_finalization_lookback {
            self.send_events(EthEvent::FinalizedBlock(bn - self.block_finalization_lookback));
        }

        // find angstrom tx
        let Some(angstrom_tx) = block
            .transactions
            .txns()
            .find(|tx| tx.to == Some(self.angstrom_contract))
        else {
            tracing::info!("No angstrom tx found");
            return
        };

        // decode call input to grab orders
        let Ok(bundle) = ContractBundle::abi_decode(&angstrom_tx.input, false) else {
            tracing::error!("failed to decode bundle");
            return
        };

        let hashes = bundle.get_filled_hashes();
        self.send_events(EthEvent::FilledOrders(hashes, bn));
        // NOTE: eoa state changes we normally get from the cannonical chain
        // update. however because of the sheer load it would take to
        // generate this manually. we will skip this for now
    }
}

impl<S: Stream<Item = Block> + Unpin + Send + 'static> Future for AnvilEthDataCleanser<S> {
    type Output = ();

    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        while let Poll::Ready(Some(block)) = self.block_subscription.poll_next_unpin(cx) {
            tracing::info!("received new block from anvil");
            self.on_new_block(block);
        }
        while let Poll::Ready(Some(cmd)) = self.commander.poll_next_unpin(cx) {
            tracing::info!("received command from chan");
            self.on_command(cmd);
        }

        Poll::Pending
    }
}
