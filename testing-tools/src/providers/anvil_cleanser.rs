use std::task::{Context, Poll};

use alloy::{primitives::Address, rpc::types::Transaction, sol_types::SolCall};
use alloy_rpc_types::TransactionTrait;
use angstrom_eth::{
    handle::{EthCommand, EthHandle},
    manager::EthEvent
};
use angstrom_types::{
    block_sync::{BlockSyncProducer, GlobalBlockSync},
    contract_payloads::angstrom::AngstromBundle,
    sol_bindings::testnet::TestnetHub
};
use futures::{Future, Stream, StreamExt};
use pade::PadeDecode;
use reth_tasks::TaskSpawner;
use tokio::sync::mpsc::{Receiver, Sender, UnboundedSender};
use tokio_stream::wrappers::ReceiverStream;
use tracing::{span, Instrument, Level};

pub struct AnvilEthDataCleanser<S: Stream<Item = (u64, Vec<Transaction>)>> {
    testnet_node_id:             u64,
    angstrom_contract:           Address,
    /// our command receiver
    commander:                   ReceiverStream<EthCommand>,
    /// people listening to events
    event_listeners:             Vec<UnboundedSender<EthEvent>>,
    block_subscription:          S,
    block_finalization_lookback: u64,
    block_sync:                  GlobalBlockSync
}

impl<S: Stream<Item = (u64, Vec<Transaction>)> + Unpin + Send + 'static> AnvilEthDataCleanser<S> {
    pub async fn spawn<TP: TaskSpawner>(
        testnet_node_id: u64,
        tp: TP,
        angstrom_contract: Address,
        tx: Sender<EthCommand>,
        rx: Receiver<EthCommand>,
        block_subscription: S,
        block_finalization_lookback: u64,
        block_sync: GlobalBlockSync
    ) -> eyre::Result<EthHandle> {
        let stream = ReceiverStream::new(rx);
        let this = Self {
            testnet_node_id,
            commander: stream,
            event_listeners: Vec::new(),
            block_subscription,
            angstrom_contract,
            block_finalization_lookback,
            block_sync
        };

        tp.spawn_critical(
            "eth handle",
            Box::pin(this.instrument(tracing::span!(
                Level::ERROR,
                "data cleanser",
                testnet_node_id
            )))
        );

        let handle = EthHandle::new(tx);

        Ok(handle)
    }

    fn send_events(&mut self, event: EthEvent) {
        self.event_listeners
            .retain(|e| e.send(event.clone()).is_ok());
    }

    fn on_command(&mut self, command: EthCommand) {
        match command {
            EthCommand::SubscribeEthNetworkEvents(tx) => self.event_listeners.push(tx),
            EthCommand::SubscribeCannon(_) => panic!("should never be called")
        }
    }

    fn on_new_block(&mut self, block: (u64, Vec<Transaction>)) {
        let (bn, txes) = block;
        self.block_sync.new_block(bn);

        self.send_events(EthEvent::NewBlock(bn));
        // no underflows today
        if bn > self.block_finalization_lookback {
            self.send_events(EthEvent::FinalizedBlock(bn - self.block_finalization_lookback));
        }

        // find angstrom tx
        let Some(angstrom_tx) = txes
            .into_iter()
            .find(|tx| tx.to() == Some(self.angstrom_contract))
        else {
            tracing::info!("No angstrom txs found");
            self.send_events(EthEvent::NewBlockTransitions {
                block_number:      block.0,
                filled_orders:     vec![],
                address_changeset: vec![]
            });

            return
        };
        let input = angstrom_tx.input();

        let Ok(bytes) = TestnetHub::executeCall::abi_decode(&input, false) else {
            tracing::warn!("found angstrom contract call thats not a bundle");
            return
        };
        let bytes = bytes.data.to_vec();
        let mut slice = bytes.as_slice();

        // decode call input to grab orders. Drop function sig
        let Ok(bundle) = AngstromBundle::pade_decode(&mut slice, None) else {
            tracing::error!("failed to decode bundle");
            return
        };

        let hashes = bundle.get_order_hashes(bn).collect::<Vec<_>>();

        let addresses = vec![];
        tracing::debug!("found angstrom tx with orders filled {:#?}", hashes);
        self.send_events(EthEvent::NewBlockTransitions {
            block_number:      block.0,
            filled_orders:     hashes,
            address_changeset: addresses
        });
    }
}

impl<S: Stream<Item = (u64, Vec<Transaction>)> + Unpin + Send + 'static> Future
    for AnvilEthDataCleanser<S>
{
    type Output = ();

    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let span = span!(Level::ERROR, "node", id = self.testnet_node_id);
        let e = span.enter();

        while let Poll::Ready(Some(block)) = self.block_subscription.poll_next_unpin(cx) {
            tracing::trace!(block_number = block.0, "received new block from anvil");
            self.on_new_block(block);
        }
        while let Poll::Ready(Some(cmd)) = self.commander.poll_next_unpin(cx) {
            tracing::trace!("received command from channel");
            self.on_command(cmd);
        }

        drop(e);

        Poll::Pending
    }
}
