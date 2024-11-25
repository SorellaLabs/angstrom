mod eth_peer;
mod strom_peer;
use std::{collections::HashSet, sync::Arc};

use alloy_chains::Chain;
use angstrom_network::{
    manager::StromConsensusEvent, state::StromState, NetworkOrderEvent, StatusState,
    StromNetworkManager, StromProtocolHandler, StromSessionManager, Swarm, VerificationSidecar
};
pub use eth_peer::*;
use parking_lot::RwLock;
use reth_chainspec::Hardforks;
use reth_metrics::common::mpsc::{MeteredPollSender, UnboundedMeteredSender};
use reth_network::test_utils::{Peer, PeerConfig};
use reth_network_peers::{pk2id, PeerId};
use reth_provider::{BlockReader, ChainSpecProvider, HeaderProvider};
use secp256k1::SecretKey;
pub use strom_peer::*;
use tokio_util::sync::PollSender;

use crate::{
    network::StromNetworkPeer,
    types::{config::TestingNodeConfig, GlobalTestingConfig}
};

pub struct TestnetNodeNetwork {
    // eth components
    pub eth_handle:   EthNetworkPeer,
    // strom components
    pub strom_handle: StromNetworkPeer,
    pub secret_key:   SecretKey,
    pub pubkey:       PeerId
}

impl TestnetNodeNetwork {
    pub async fn new<C, G>(
        c: C,
        node_config: &TestingNodeConfig<G>,
        to_pool_manager: Option<UnboundedMeteredSender<NetworkOrderEvent>>,
        to_consensus_manager: Option<UnboundedMeteredSender<StromConsensusEvent>>
    ) -> (Self, Peer<C>, StromNetworkManager<C>)
    where
        C: BlockReader
            + HeaderProvider
            + ChainSpecProvider
            + Unpin
            + Clone
            + ChainSpecProvider<ChainSpec: Hardforks>
            + 'static,
        G: GlobalTestingConfig
    {
        let sk = node_config.secret_key.clone();
        let peer = PeerConfig::with_secret_key(c.clone(), sk.clone());

        let peer_id = pk2id(&node_config.pub_key);
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
            secret_key:   sk.clone()
        };

        let validators = Arc::new(RwLock::new(HashSet::default()));

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

        (
            Self { strom_handle, secret_key: sk.clone(), pubkey: peer_id, eth_handle },
            eth_peer,
            strom_network
        )
    }

    pub fn pubkey(&self) -> PeerId {
        self.pubkey
    }
}
