mod consensus_future;
pub(crate) use consensus_future::TestnetConsensusFuture;
mod eth_peer;
mod network_future;
mod strom_peer;
use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc
    },
    task::Poll
};

use alloy_chains::Chain;
use alloy_primitives::Address;
use angstrom_network::{
    manager::StromConsensusEvent, state::StromState, NetworkOrderEvent, StatusState,
    StromNetworkManager, StromProtocolHandler, StromSessionManager, Swarm, VerificationSidecar
};
pub use eth_peer::*;
use network_future::TestnetPeerStateFuture;
use parking_lot::RwLock;
use reth_chainspec::Hardforks;
use reth_metrics::common::mpsc::{MeteredPollSender, UnboundedMeteredSender};
use reth_network::test_utils::PeerConfig;
use reth_network_peers::{pk2id, PeerId};
use reth_provider::{BlockReader, ChainSpecProvider, HeaderProvider};
use secp256k1::{PublicKey, SecretKey};
pub use strom_peer::*;
use tokio_util::sync::PollSender;

use crate::network::StromNetworkPeer;

pub struct TestnetNodeNetwork<C> {
    // eth components
    pub eth_handle:      EthNetworkPeer,
    // strom components
    pub strom_handle:    StromNetworkPeer,
    pub secret_key:      SecretKey,
    pub pubkey:          PeerId,
    running:             Arc<AtomicBool>,
    pub(crate) networks: TestnetPeerStateFuture<C>
}

impl<C> TestnetNodeNetwork<C>
where
    C: BlockReader
        + HeaderProvider
        + ChainSpecProvider
        + Unpin
        + Clone
        + ChainSpecProvider<ChainSpec: Hardforks>
        + 'static
{
    pub async fn new_fully_configed(
        testnet_node_id: u64,
        c: C,
        pub_key: PublicKey,
        sk: SecretKey,
        to_pool_manager: Option<UnboundedMeteredSender<NetworkOrderEvent>>,
        to_consensus_manager: Option<UnboundedMeteredSender<StromConsensusEvent>>
    ) -> Self {
        let peer = PeerConfig::with_secret_key(c.clone(), sk);

        let peer_id = pk2id(&pub_key);
        let state = StatusState {
            version:   0,
            chain:     Chain::mainnet().id(),
            peer:      peer_id,
            timestamp: 0
        };
        let (session_manager_tx, session_manager_rx) = tokio::sync::mpsc::channel(100);
        let sidecar = VerificationSidecar {
            status:       state,
            has_sent:     false,
            has_received: false,
            secret_key:   sk
        };

        let validators: HashSet<Address> = HashSet::default();
        let validators = Arc::new(RwLock::new(validators));

        let protocol = StromProtocolHandler::new(
            MeteredPollSender::new(PollSender::new(session_manager_tx), "session manager"),
            sidecar,
            validators.clone()
        );

        let state = StromState::new(c.clone(), validators.clone());
        let sessions = StromSessionManager::new(session_manager_rx);
        let swarm = Swarm::new(sessions, state);

        let strom_network = StromNetworkManager::new(swarm, to_pool_manager, to_consensus_manager);

        let mut eth_peer = peer.launch().await.unwrap();
        eth_peer.network_mut().add_rlpx_sub_protocol(protocol);

        let strom_handle = StromNetworkPeer::new(&strom_network);
        let eth_handle = EthNetworkPeer::new(&eth_peer);

        let running = Arc::new(AtomicBool::new(true));
        let futs =
            TestnetPeerStateFuture::new(testnet_node_id, eth_peer, strom_network, running.clone());

        Self { strom_handle, secret_key: sk, pubkey: peer_id, networks: futs, eth_handle, running }
    }

    pub fn pubkey(&self) -> PeerId {
        self.pubkey
    }

    pub fn stop_network(&self) {
        self.running.store(false, Ordering::Relaxed);
    }

    pub fn blocking_stop_network(&self) {
        self.running.store(false, Ordering::Relaxed);
        while self.is_network_on() {}
    }

    pub fn start_network(&self) {
        self.running.store(true, Ordering::Relaxed);
    }

    pub fn blocking_start_network(&self) {
        self.running.store(true, Ordering::Relaxed);
        while self.is_network_off() {}
    }

    pub fn is_network_off(&self) -> bool {
        !self.running.load(Ordering::Relaxed)
    }

    pub fn is_network_on(&self) -> bool {
        !self.running.load(Ordering::Relaxed)
    }

    pub(crate) async fn initialize_connections(&mut self, connections_needed: usize) {
        tracing::debug!(pubkey = ?self.pubkey, "attempting connections to {connections_needed} peers");
        let mut last_peer_count = 0;
        std::future::poll_fn(|cx| loop {
            if self.networks.poll_fut_to_initialize(cx).is_ready() {
                panic!("peer connection failed");
            }

            let peer_cnt = self.strom_handle.peer_count();
            if last_peer_count != peer_cnt {
                tracing::trace!("connected to {peer_cnt}/{connections_needed} peers");
                last_peer_count = peer_cnt;
            }

            if connections_needed == peer_cnt {
                return Poll::Ready(())
            }
        })
        .await
    }
}
