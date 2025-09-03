mod devnet;
mod op_testnet;
mod state_machine;
use std::{
    collections::{HashMap, HashSet},
    future::Future
};

use alloy::{node_bindings::AnvilInstance, providers::Provider};
use angstrom_types::block_sync::GlobalBlockSync;
use futures::TryFutureExt;
use rand::Rng;
pub use state_machine::*;
use tracing::{Instrument, Level, span};

use crate::{
    controllers::strom::OpTestnetNode,
    providers::{AnvilProvider, TestnetBlockProvider, utils::async_to_sync},
    types::{GlobalTestingConfig, WithWalletProvider}
};

pub struct OpAngstromTestnet<G, P> {
    block_provider:      TestnetBlockProvider,
    _anvil_instance:     Option<AnvilInstance>,
    peers:               HashMap<u64, OpTestnetNode<P, G>>,
    current_max_peer_id: u64,
    block_syncs:         Vec<GlobalBlockSync>,
    config:              G
}

impl<G, P> OpAngstromTestnet<G, P>
where
    G: GlobalTestingConfig,
    P: WithWalletProvider
{
    pub fn node_provider(&self, node_id: Option<u64>) -> &AnvilProvider<P> {
        self.peers
            .get(&node_id.unwrap_or_default())
            .unwrap()
            .state_provider()
    }

    pub fn random_peer(&self) -> &OpTestnetNode<P, G> {
        let mut rng = rand::rng();
        let peer = rng.random_range(0..self.current_max_peer_id);
        self.get_peer(peer)
    }

    fn random_valid_id(&self) -> u64 {
        let ids = self.peers.keys().copied().collect::<Vec<_>>();
        let id_idx = rand::rng().random_range(0..ids.len());
        ids[id_idx]
    }

    pub fn get_peer(&self, id: u64) -> &OpTestnetNode<P, G> {
        self.peers
            .get(&id)
            .unwrap_or_else(|| panic!("peer {id} not found"))
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

    /// if id is None, then a random id is used
    async fn run_event<'a, F, O>(&'a self, id: Option<u64>, f: F) -> O::Output
    where
        F: FnOnce(&'a OpTestnetNode<P, G>) -> O,
        O: Future + Send + Sync
    {
        let id = if let Some(i) = id {
            assert!(!self.peers.is_empty());
            assert!(
                self.peers
                    .keys()
                    .copied()
                    .collect::<HashSet<_>>()
                    .contains(&i)
            );
            i
        } else {
            self.random_valid_id()
        };

        let peer = self.peers.get(&id).unwrap();
        let span = span!(Level::ERROR, "testnet node", ?id);
        f(peer).instrument(span).await
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
