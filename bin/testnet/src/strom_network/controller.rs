use std::{
    collections::{HashMap, HashSet},
    future::Future,
    task::Poll
};

use futures::FutureExt;
use rand::Rng;
use reth_primitives::Address;
use reth_provider::{test_utils::NoopProvider, BlockReader, HeaderProvider};
use tracing::{span, Instrument, Level};

use super::manager::{TestnetPeerManager, TestnetPeerManagerBuilder};
use crate::{
    cli::Cli, contract_setup::deploy_contract_and_create_pool, eth::RpcStateProviderFactoryWrapper
};

pub struct StromController<C = NoopProvider> {
    peers:              HashMap<u64, TestnetPeerManager<C>>,
    disconnected_peers: Vec<u64>
}

impl<C> StromController<C>
where
    C: BlockReader + HeaderProvider + Unpin + Clone + Default + 'static
{
    pub fn new() -> Self {
        Self { peers: Default::default(), disconnected_peers: Vec::new() }
    }

    pub async fn spawn_testnet_framework(cli: Cli) -> eyre::Result<Self> {
        let mut angr_addr = Address::default();
        let mut all_nodes = Vec::new();
        for id in 0..cli.nodes_in_network {
            let span = span!(Level::TRACE, "testnet node", id);
            let (node, addr) = Self::build_peer(id, cli.starting_port, cli.testnet_block_time_secs)
                .instrument(span)
                .await?;
            angr_addr = addr;
            all_nodes.push(node);
        }

        Self::connect_all_peers(&mut all_nodes).await;
        tracing::info!("finished connecting peers");

        let mut this = Self::new();

        for peer in all_nodes {
            this.spawn_testnet_node(peer, angr_addr).await?;
        }

        Ok(this)
    }

    async fn build_peer(
        id: u64,
        starting_port: u16,
        testnet_block_time_secs: u64
    ) -> eyre::Result<(TestnetPeerManagerBuilder<C>, Address)> {
        tracing::debug!("connecting to state provider");
        let rpc_wrapper =
            RpcStateProviderFactoryWrapper::spawn_new(testnet_block_time_secs, id).await?;
        tracing::info!("connected to state provider");

        tracing::debug!("deploying contracts to anvil");
        let addresses = deploy_contract_and_create_pool(rpc_wrapper.provider().provider()).await?;
        tracing::info!("deployed contracts to anvil");

        let angstrom_addr = addresses.contract;

        let peer =
            TestnetPeerManagerBuilder::new(id, starting_port as u64, C::default(), rpc_wrapper)
                .await;

        Ok((peer, angstrom_addr))
    }

    async fn connect_all_peers(peer_set: &mut [TestnetPeerManagerBuilder<C>]) {
        let these_peers = peer_set.iter().collect::<Vec<_>>();

        for peer in &*peer_set {
            for other_peer in &these_peers {
                if *peer.public_key == *other_peer.public_key {
                    continue;
                }
                peer.peer.add_validator(other_peer.public_key)
            }
        }
        // add all peers to each other
        for (idx, peer) in peer_set.iter().enumerate().take(peer_set.len() - 1) {
            for other_peer in peer_set.iter().skip(idx + 1) {
                peer.peer
                    .connect_to_peer(other_peer.public_key, other_peer.peer.socket_addr());
            }
        }

        // // wait on each peer to add all other peers
        // let needed_peers = peer_set.len() - 1;
        // let streams = self.peers.iter().map(|(id, peer_handle)| {
        //     (*id, NetworkEventStream::new(peer_handle.peer.eth_network_handle().
        // event_listener())) });

        // // await all sessions to be established
        // let fut = streams.into_iter().map(|(id, mut stream)| {
        //     let span = span!(Level::DEBUG, "testnet node", id);
        //     async move {
        //         let connected = stream.take_session_established(needed_peers).await;

        //         tracing::info!("connected to {} peers", connected.len());
        //     }
        //     .instrument(span)
        // });

        // futures::future::join_all(fut).await;

        let needed_peers = peer_set.len() - 1;
        let mut peers = peer_set
            .iter_mut()
            .map(|p| (p.id, &mut p.peer))
            .collect::<Vec<_>>();

        // std::future::poll_fn(|cx| {
        //     let mut all_connected = true;
        //     for (id, peer) in &mut peers {
        //         let span = span!(Level::TRACE, "testnet node", ?id);
        //         let e = span.enter();

        //         if peer.poll_connect(cx, needed_peers) {
        //             tracing::error!("peer failed");
        //         } else {
        //             tracing::trace!("connected to {needed_peers} peers");
        //         }
        //         drop(e);

        //         // all_connected &= peer.get_peer_count() == needed_peers;
        //     }

        //     //    if all_connected {
        //     return Poll::Ready(())
        //     //  }

        //     // Poll::Pending
        // })
        // .await

        tokio::spawn(std::future::poll_fn(|cx| {
            //loop {
            let mut all_connected = true;
            for (id, peer) in &mut peers {
                // let span = span!(Level::TRACE, "testnet node");
                // let e = span.enter();
                if peer.poll_unpin(cx).is_ready() {
                    tracing::error!("peer failed");
                }

                tracing::trace!("connected to {}/{needed_peers} peers", peer.get_peer_count());

                all_connected &= peer.get_peer_count() == needed_peers;
                //drop(e);
            }

            if all_connected {
                return Poll::Ready(())
            }
            // }
            Poll::Pending
        }))
        .await;
    }

    /// if None, then a random id is used
    pub async fn run_event<'a, F, O, R>(&'a self, id: Option<u64>, f: F) -> R
    where
        F: FnOnce(&'a TestnetPeerManager<C>) -> O,
        O: Future<Output = R>
    {
        let id = if let Some(i) = id {
            assert!(!self.peers.is_empty());
            assert!(self
                .peers
                .iter()
                .map(|(id, _)| *id)
                .collect::<HashSet<_>>()
                .contains(&i));
            i
        } else {
            self.get_random_id()
        };

        let peer = self.peers.get(&id).unwrap();
        let span = span!(Level::DEBUG, "testnet node", ?id);
        let r = f(&peer).instrument(span).await;
        r
    }

    fn add_peer(&mut self, peer: TestnetPeerManager<C>) {
        self.peers.insert(peer.id, peer);
    }

    pub fn get_peer(&self, id: u64) -> &TestnetPeerManager<C> {
        self.peers.get(&id).expect(&format!("peer {id} not found"))
    }

    async fn spawn_testnet_node(
        &mut self,
        peer_builder: TestnetPeerManagerBuilder<C>,
        contract_address: Address
    ) -> eyre::Result<()> {
        let peer = peer_builder.build_and_spawn(contract_address).await?;

        self.add_peer(peer);

        Ok(())
    }

    fn get_random_id(&self) -> u64 {
        let ids = self.peers.iter().map(|(id, _)| *id).collect::<Vec<_>>();
        let id_idx = rand::thread_rng().gen_range(0..ids.len());
        ids[id_idx]
    }

    /// updates the anvil state of all the peers from a given peer from a given
    /// peer. Returns the latest block
    pub async fn update_state(&self, id: u64) -> eyre::Result<()> {
        let peer = self
            .peers
            .get(&id)
            .expect(&format!("peer {id} doesn't exist"));
        let (updated_state, _) = peer.rpc_wrapper.execute_and_return_state().await?;

        futures::future::join_all(self.peers.iter().map(|(i, peer)| async {
            if id != *i {
                peer.rpc_wrapper.set_state(updated_state.clone()).await?;
            }
            Ok::<_, eyre::ErrReport>(())
        }))
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;

        Ok(())
    }
}
