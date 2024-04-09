use std::{collections::HashMap, task::Poll};
mod strom_peer;
use angstrom_network::StromNetworkEvent;
use futures::{stream::StreamExt, FutureExt};
use reth_primitives::*;
use reth_provider::test_utils::NoopProvider;
use secp256k1::SecretKey;
use tokio_stream::wrappers::UnboundedReceiverStream;

use self::strom_peer::StromPeer;

/// the goal of the angstrom testnet is to extend reth's baseline tests
/// as-well as expand appon to allow for composing tests and ensuring full
/// performance
pub struct AngstromTestnet {
    pub peers:       HashMap<PeerId, StromPeer>,
    pub peer_events: HashMap<PeerId, UnboundedReceiverStream<StromNetworkEvent>>
}

impl AngstromTestnet {
    pub async fn new(peers: usize, provider: NoopProvider) -> Self {
        let peers = futures::stream::iter(0..peers)
            .map(|_| async move {
                let peer = StromPeer::new(provider.clone()).await;
                let pk = peer.get_node_public_key();
                (pk, peer)
            })
            .buffer_unordered(4)
            .collect::<HashMap<_, _>>()
            .await;

        let peer_events = peers
            .iter()
            .map(|(k, p)| (*k, p.sub_network_events()))
            .collect::<HashMap<_, _>>();

        Self { peers, peer_events }
    }

    pub fn add_new_peer(&mut self, peer: StromPeer) {
        let pk = peer.get_node_public_key();
        self.peers.insert(pk, peer);
    }

    pub fn peers(&self) -> impl Iterator<Item = (&PeerId, &StromPeer)> + '_ {
        self.peers.iter()
    }

    pub fn peers_mut(&mut self) -> impl Iterator<Item = (&PeerId, &mut StromPeer)> + '_ {
        self.peers.iter_mut()
    }

    /// ensures all peers have eachother on there validator list
    pub async fn connect_all_peers(&mut self) {
        let peer_set = self.peers.keys().collect::<Vec<_>>();
        for (pk, peer) in &self.peers {
            for other in &peer_set {
                if pk == *other {
                    continue
                }
                peer.add_validator(**other)
            }
        }

        // wait on each peer to add all other peers
        let needed_peers = self.peers.len() - 1;
        let mut peers = self.peers.iter_mut().map(|(_, p)| p).collect::<Vec<_>>();
        let mut chans = self.peer_events.values_mut().collect::<Vec<_>>();

        std::future::poll_fn(|cx| {
            let mut all_connected = true;
            tracing::info!("pollin");
            for peer in &mut peers {
                if let Poll::Ready(_) = peer.poll_unpin(cx) {
                    tracing::error!("peer failed");
                }
                all_connected &= peer.get_peer_count() == needed_peers
            }

            for chan in &mut chans {
                if let Poll::Ready(Some(msg)) = chan.poll_next_unpin(cx) {
                    tracing::debug!(?msg, "peer got msg");
                }
            }
            if all_connected {
                return Poll::Ready(())
            }

            Poll::Pending
        })
        .await
    }

    /// returns the next event that any peer emits
    pub async fn progress_to_next_event(&mut self) -> StromNetworkEvent {
        let mut subscriptions = self
            .peers()
            .map(|(_, peer)| peer.sub_network_events())
            .collect::<Vec<_>>();

        std::future::poll_fn(|cx| {
            for sub in &mut subscriptions {
                if let Poll::Ready(Some(res)) = sub.poll_next_unpin(cx) {
                    return Poll::Ready(res)
                }
            }

            Poll::Pending
        })
        .await
    }
}
