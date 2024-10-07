use std::{
    collections::HashSet,
    marker::PhantomData,
    net::SocketAddr,
    sync::Arc,
    task::{Context, Poll}
};

use alloy_chains::Chain;
use angstrom_network::{
    manager::StromConsensusEvent, state::StromState, NetworkOrderEvent, StatusState,
    StromNetworkEvent, StromNetworkHandle, StromNetworkManager, StromProtocolHandler,
    StromSessionManager, Swarm, VerificationSidecar
};
use futures::{Future, FutureExt};
use parking_lot::RwLock;
use rand::thread_rng;
use reth_metrics::common::mpsc::{MeteredPollSender, UnboundedMeteredSender};
use reth_network::test_utils::{Peer, PeerConfig, PeerHandle};
use reth_network_api::Peers;
use reth_network_peers::{pk2id, PeerId};
use reth_primitives::Address;
use reth_provider::{test_utils::NoopProvider, BlockReader, HeaderProvider};
use reth_transaction_pool::{
    blobstore::InMemoryBlobStore, noop::MockTransactionValidator, test_utils::MockTransaction,
    CoinbaseTipOrdering, Pool
};
use secp256k1::{PublicKey, Secp256k1, SecretKey};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_util::sync::PollSender;
use tracing::{span, Instrument, Level, Span};

type PeerPool = Pool<
    MockTransactionValidator<MockTransaction>,
    CoinbaseTipOrdering<MockTransaction>,
    InMemoryBlobStore
>;

struct EthPeer {
    peer_fut:        JoinHandle<()>,
    /// the default ethereum network peer
    eth_peer_handle: PeerHandle<PeerPool>
}

struct StromPeer<C = NoopProvider> {
    strom_network_fut:    JoinHandle<()>,
    strom_network_handle: StromNetworkHandle,
    strom_validator_set:  Arc<RwLock<HashSet<Address>>>,
    _phantom:             PhantomData<C> // can't be asked to move the generics everywhere
}

pub struct TestnetPeer<C = NoopProvider> {
    eth:            EthPeer,
    // strom extensions
    strom:          StromPeer<C>,
    pub secret_key: SecretKey,
    pub peer_id:    PeerId,
    _span:          Span
}

impl<C: Unpin> TestnetPeer<C>
where
    C: BlockReader + HeaderProvider + Unpin + Clone + 'static
{
    pub async fn new(id: u64, c: C) -> Self {
        let mut rng = thread_rng();
        let sk = SecretKey::new(&mut rng);
        let peer = PeerConfig::with_secret_key(c.clone(), sk);

        let secp = Secp256k1::default();
        let pub_key = sk.public_key(&secp);

        let peer_id = pk2id(&pub_key);
        let state = StatusState {
            version:   0,
            chain:     Chain::mainnet().id(),
            peer:      peer_id,
            timestamp: 0
        };

        let validators: HashSet<Address> = HashSet::default();
        let validator_set = Arc::new(RwLock::new(validators));

        let verification = VerificationSidecar {
            status:       state,
            has_sent:     false,
            has_received: false,
            secret_key:   sk
        };

        let (session_manager_tx, session_manager_rx) = tokio::sync::mpsc::channel(100);

        let protocol = StromProtocolHandler::new(
            MeteredPollSender::new(PollSender::new(session_manager_tx), "session manager"),
            verification.clone(),
            validator_set.clone()
        );

        let state = StromState::new(c.clone(), validator_set.clone());
        let sessions = StromSessionManager::new(session_manager_rx);
        let swarm = Swarm::new(sessions, state);

        let network = StromNetworkManager::new(swarm, None, None);
        let mut peer = peer.launch().await.unwrap();
        peer.network_mut().add_rlpx_sub_protocol(protocol);
        let handle = network.get_handle();

        let span = span!(Level::DEBUG, "testnet node", id);

        Self {
            eth: EthPeer {
                eth_peer_handle: peer.peer_handle(),
                peer_fut:        tokio::spawn(peer.instrument(span.clone()))
            },
            strom: StromPeer {
                strom_validator_set:  network.swarm().state().validators().clone(),
                strom_network_fut:    tokio::spawn(network.instrument(span.clone())),
                strom_network_handle: handle,
                _phantom:             PhantomData
            },
            secret_key: sk,
            peer_id,
            _span: span
        }
    }

    pub fn new_with_consensus() -> Self {
        todo!("consensus not configured for test peer")
    }

    pub fn new_with_tx_pool() -> Self {
        todo!("tx pool not configured for test peer")
    }

    pub async fn new_fully_configed(
        id: u64,
        c: C,
        to_pool_manager: Option<UnboundedMeteredSender<NetworkOrderEvent>>,
        to_consensus_manager: Option<UnboundedMeteredSender<StromConsensusEvent>>
    ) -> Self {
        let mut rng = thread_rng();
        let sk = SecretKey::new(&mut rng);
        let peer = PeerConfig::with_secret_key(c.clone(), sk);

        let secp = Secp256k1::default();
        let pub_key = sk.public_key(&secp);

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

        let network = StromNetworkManager::new(swarm, to_pool_manager, to_consensus_manager);

        let mut peer = peer.launch().await.unwrap();
        peer.network_mut().add_rlpx_sub_protocol(protocol);
        let handle = network.get_handle();

        let span = span!(Level::DEBUG, "testnet node", id);

        Self {
            eth: EthPeer {
                eth_peer_handle: peer.peer_handle(),
                peer_fut:        tokio::spawn(peer.instrument(span.clone()))
            },
            strom: StromPeer {
                strom_validator_set:  network.swarm().state().validators().clone(),
                strom_network_fut:    tokio::spawn(network.instrument(span.clone())),
                strom_network_handle: handle,
                _phantom:             PhantomData
            },
            secret_key: sk,
            peer_id,
            _span: span
        }
    }

    pub fn strom_network_handle(&self) -> &StromNetworkHandle {
        &self.strom.strom_network_handle
    }

    pub fn eth_network_handle(&self) -> &PeerHandle<PeerPool> {
        &self.eth.eth_peer_handle
    }

    pub fn get_node_public_key(&self) -> PeerId {
        let pub_key = PublicKey::from_secret_key(&Secp256k1::default(), &self.secret_key);
        pk2id(&pub_key)
    }

    pub fn disconnect_peer(&self, id: PeerId) {
        self.strom_network_handle().remove_peer(id)
    }

    pub fn get_peer_count(&self) -> usize {
        self.strom_network_handle().peer_count()
    }

    pub fn remove_validator(&self, id: PeerId) {
        let addr = Address::from_raw_public_key(id.as_slice());
        let set = self.get_validator_set();
        set.write().remove(&addr);
    }

    pub fn add_validator(&self, id: PeerId) {
        let addr = Address::from_raw_public_key(id.as_slice());
        let set = self.get_validator_set();
        set.write().insert(addr);
    }

    pub fn connect_to_peer(&self, id: PeerId, addr: SocketAddr) {
        self.eth_network_handle().peer_handle().add_peer(id, addr);
    }

    pub fn socket_addr(&self) -> SocketAddr {
        self.eth_network_handle().local_addr()
    }

    fn get_validator_set(&self) -> Arc<RwLock<HashSet<Address>>> {
        self.strom.strom_validator_set.clone()
    }

    pub fn sub_network_events(&self) -> UnboundedReceiverStream<StromNetworkEvent> {
        self.strom_network_handle().subscribe_network_events()
    }

    pub fn removed_peer(&self) {
        self.strom.strom_network_fut.abort();
        self.eth.peer_fut.abort();
    }

    pub fn manual_poll(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        if self.strom.strom_network_fut.poll_unpin(cx).is_ready()
            || self.eth.peer_fut.poll_unpin(cx).is_ready()
        {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

// impl<C> Future for TestnetPeer<C>
// where
//     C: BlockReader + HeaderProvider + Unpin + Clone + 'static
// {
//     type Output = ();

//     fn poll(
//         self: std::pin::Pin<&mut Self>,
//         cx: &mut std::task::Context<'_>
//     ) -> std::task::Poll<Self::Output> {
//         let this = self.get_mut();
//         let peer_id = this.get_node_public_key();
//         let span = span!(Level::TRACE, "peer_id: {:?}", ?peer_id);
//         let e = span.enter();

//         if this.strom_network.poll_unpin(cx).is_ready() {
//             return Poll::Ready(())
//         }

//         drop(e);

//         Poll::Pending
//     }
// }
