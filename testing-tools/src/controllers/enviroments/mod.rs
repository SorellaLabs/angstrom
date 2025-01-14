mod devnet;
mod state_machine;
mod testnet;
use std::{
    collections::{HashMap, HashSet},
    future::Future
};

use alloy::{node_bindings::AnvilInstance, providers::Provider};
use angstrom_network::{
    manager::StromConsensusEvent, NetworkOrderEvent, StromMessage, StromNetworkManager
};
use angstrom_types::sol_bindings::grouped_orders::AllOrders;
use futures::TryFutureExt;
use rand::Rng;
use reth_chainspec::Hardforks;
use reth_metrics::common::mpsc::{
    metered_unbounded_channel, UnboundedMeteredReceiver, UnboundedMeteredSender
};
use reth_provider::{BlockReader, ChainSpecProvider, HeaderProvider, ReceiptProvider};
pub use state_machine::*;
use tokio_stream::StreamExt;
use tracing::{span, Instrument, Level};

use crate::{
    controllers::strom::TestnetNode,
    providers::{utils::async_to_sync, TestnetBlockProvider},
    types::{GlobalTestingConfig, WithWalletProvider}
};

pub struct AngstromTestnet<C, G, P> {
    block_provider:      TestnetBlockProvider,
    _anvil_instance:     Option<AnvilInstance>,
    peers:               HashMap<u64, TestnetNode<C, P>>,
    _disconnected_peers: HashSet<u64>,
    _dropped_peers:      HashSet<u64>,
    current_max_peer_id: u64,
    config:              G
}

impl<C, G, P> AngstromTestnet<C, G, P>
where
    C: BlockReader<Block = reth_primitives::Block>
        + ReceiptProvider<Receipt = reth_primitives::Receipt>
        + HeaderProvider<Header = reth_primitives::Header>
        + ChainSpecProvider<ChainSpec: Hardforks>
        + Unpin
        + Clone
        + 'static,
    G: GlobalTestingConfig,
    P: WithWalletProvider
{
    pub fn random_peer(&self) -> &TestnetNode<C, P> {
        let mut rng = rand::thread_rng();
        let peer = rng.gen_range(0..self.current_max_peer_id);
        self.get_peer(peer)
    }

    /// increments the `current_max_peer_id` and returns the previous value
    fn incr_peer_id(&mut self) -> u64 {
        let prev_id = self.current_max_peer_id;
        self.current_max_peer_id += 1;
        prev_id
    }

    fn random_valid_id(&self) -> u64 {
        let ids = self.peers.keys().copied().collect::<Vec<_>>();
        let id_idx = rand::thread_rng().gen_range(0..ids.len());
        ids[id_idx]
    }

    pub fn get_peer(&self, id: u64) -> &TestnetNode<C, P> {
        self.peers
            .get(&id)
            .unwrap_or_else(|| panic!("peer {id} not found"))
    }

    fn get_peer_mut(&mut self, id: u64) -> &mut TestnetNode<C, P> {
        self.peers
            .get_mut(&id)
            .unwrap_or_else(|| panic!("peer {id} not found"))
    }

    pub fn get_random_peer(&self, not_allowed_ids: Vec<u64>) -> &TestnetNode<C, P> {
        assert!(!self.peers.is_empty());

        let peer_ids = self
            .peers
            .keys()
            .copied()
            .filter(|id| !not_allowed_ids.contains(id))
            .collect::<Vec<_>>();

        if peer_ids.is_empty() {
            panic!("not enough peers")
        }

        let mut random_peer = self.random_valid_id();
        while !peer_ids.contains(&random_peer) {
            random_peer = self.random_valid_id();
        }

        self.peers
            .get(&random_peer)
            .unwrap_or_else(|| panic!("peer {random_peer} not found"))
    }

    /// updates the anvil state of all the peers from a given peer
    pub(crate) async fn all_peers_update_state(&self, id: u64) -> eyre::Result<()> {
        let peer = self.get_peer(id);
        let (updated_state, block) = peer.state_provider().execute_and_return_state().await?;
        self.block_provider.broadcast_block(block);

        futures::future::join_all(self.peers.iter().map(|(i, peer)| async {
            if id != *i {
                peer.state_provider()
                    .set_state(updated_state.clone())
                    .await?;
            }
            Ok::<_, eyre::ErrReport>(())
        }))
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;

        Ok(())
    }

    /// updates the anvil state of all the peers from a given peer
    pub(crate) async fn single_peer_update_state(
        &self,
        state_from_id: u64,
        state_to_id: u64
    ) -> eyre::Result<()> {
        let peer_to_get = self.get_peer(state_from_id);
        let state = peer_to_get.state_provider().return_state().await?;

        let peer_to_set = self.peers.get(&state_to_id).expect("peer doesn't exists");
        peer_to_set.state_provider().set_state(state).await?;

        Ok(())
    }

    /// takes a random peer and gets them to broadcast the message. we then
    /// take all other peers and ensure that they received the message.
    pub async fn broadcast_orders_message(
        &mut self,
        id: Option<u64>,
        sent_msg: StromMessage,
        expected_orders: Vec<AllOrders>
    ) -> eyre::Result<bool> {
        let out = self
            .run_network_event_on_all_peers_with_exception(
                id.unwrap_or_else(|| self.random_valid_id()),
                |peer| {
                    let network_handle = peer.strom_network_handle().clone();
                    let peer_id = peer.peer_id();

                    async move {
                        network_handle.broadcast_message(sent_msg.clone());
                        peer_id
                    }
                },
                |other_rxs, peer_id| async move {
                    futures::future::join_all(other_rxs.into_iter().map(|mut rx| {
                        let value = expected_orders.clone();
                        async move {
                            (Some(NetworkOrderEvent::IncomingOrders { peer_id, orders: value })
                                == rx.next().await) as usize
                        }
                    }))
                    .await
                    .into_iter()
                    .sum::<usize>()
                },
                |manager, tx| manager.swap_pool_manager(tx)
            )
            .await;

        Ok(out == self.peers.len() - 1)
    }

    /// takes a random peer and gets them to broadcast the message. we then
    /// take all other peers and ensure that they received the message.
    pub async fn broadcast_consensus_message(
        &mut self,
        id: Option<u64>,
        sent_msg: StromMessage,
        expected_message: StromConsensusEvent
    ) -> eyre::Result<bool> {
        let out = self
            .run_network_event_on_all_peers_with_exception(
                id.unwrap_or_else(|| self.random_valid_id()),
                |peer| {
                    let network_handle = peer.strom_network_handle().clone();
                    let peer_id = peer.peer_id();

                    async move {
                        network_handle.broadcast_message(sent_msg.clone());
                        peer_id
                    }
                },
                |other_rxs, _| async move {
                    futures::future::join_all(other_rxs.into_iter().map(|mut rx| {
                        let value = expected_message.clone();
                        async move { (Some(value) == rx.next().await) as usize }
                    }))
                    .await
                    .into_iter()
                    .sum::<usize>()
                },
                |manager, tx| manager.swap_consensus_manager(tx)
            )
            .await;

        Ok(out == self.peers.len() - 1)
    }

    /// if id is None, then a random id is used
    async fn run_event<'a, F, O>(&'a self, id: Option<u64>, f: F) -> O::Output
    where
        F: FnOnce(&'a TestnetNode<C, P>) -> O,
        O: Future + Send + Sync
    {
        let id = if let Some(i) = id {
            assert!(!self.peers.is_empty());
            assert!(self
                .peers
                .keys()
                .copied()
                .collect::<HashSet<_>>()
                .contains(&i));
            i
        } else {
            self.random_valid_id()
        };

        let peer = self.peers.get(&id).unwrap();
        let span = span!(Level::ERROR, "testnet node", ?id);
        f(peer).instrument(span).await
    }

    /// runs an event that uses the consensus or orderpool channels in the
    /// angstrom network and compares a expected result against all peers
    async fn run_network_event_on_all_peers_with_exception<F, K, O, R, E>(
        &mut self,
        exception_id: u64,
        network_f: F,
        expected_f: K,
        channel_swap_f: impl Fn(
            &mut StromNetworkManager<C>,
            UnboundedMeteredSender<E>
        ) -> Option<UnboundedMeteredSender<E>>
    ) -> R::Output
    where
        F: FnOnce(&TestnetNode<C, P>) -> O,
        K: FnOnce(Vec<UnboundedMeteredReceiver<E>>, O::Output) -> R,
        O: Future + Send + Sync,
        R: Future + Send + Sync
    {
        let (old_peer_channels, rx_channels): (Vec<_>, Vec<_>) = self
            .peers
            .iter_mut()
            .filter(|(id, _)| **id != exception_id)
            .map(|(id, peer)| {
                let (new_tx, new_rx) = metered_unbounded_channel("new orderpool");
                let old_tx = peer
                    .pre_post_network_event_channel_swap(true, |net| channel_swap_f(net, new_tx));

                ((*id, old_tx), new_rx)
            })
            .unzip();

        let event_out = self.run_event(Some(exception_id), network_f).await;

        self.peers
            .iter()
            .filter(|(id, _)| **id != exception_id)
            .for_each(|(_, peer)| {
                peer.start_network();
            });

        let out = expected_f(rx_channels, event_out).await;

        old_peer_channels.into_iter().for_each(|(id, old_tx)| {
            let peer = self.get_peer_mut(id);
            let _ =
                peer.pre_post_network_event_channel_swap(false, |net| channel_swap_f(net, old_tx));
        });

        out
    }

    /// checks the current block number on all peers matches the expected
    pub(crate) fn check_block_numbers(&self, expected_block_num: u64) -> eyre::Result<bool> {
        let f = self.peers.values().map(|peer| {
            let id = peer.testnet_node_id();
            peer.state_provider()
                .rpc_provider()
                .get_block_number()
                .and_then(move |r| async move { Ok((id, r)) })
        });

        let blocks = async_to_sync(futures::future::join_all(f))
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;

        Ok(blocks.into_iter().all(|(_, b)| b == expected_block_num))
    }
}
