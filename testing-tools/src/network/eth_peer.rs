use std::net::SocketAddr;

use angstrom_types::primitive::PeerId;
use reth_network::{
    test_utils::{Peer, PeerHandle},
    NetworkHandle, NetworkInfo, Peers
};
use reth_provider::{BlockReader, HeaderProvider};
use reth_transaction_pool::{
    blobstore::InMemoryBlobStore, noop::MockTransactionValidator, test_utils::MockTransaction,
    CoinbaseTipOrdering, Pool
};

pub type EthPeerPool = Pool<
    MockTransactionValidator<MockTransaction>,
    CoinbaseTipOrdering<MockTransaction>,
    InMemoryBlobStore
>;

pub struct EthNetworkPeer {
    peer_handle:    PeerHandle<EthPeerPool>,
    network_handle: NetworkHandle
}

impl EthNetworkPeer {
    pub fn new<C>(peer: &Peer<C>) -> Self
    where
        C: BlockReader + HeaderProvider + Clone + 'static
    {
        Self { peer_handle: peer.peer_handle(), network_handle: peer.handle() }
    }

    pub fn peer_handle(&self) -> &PeerHandle<EthPeerPool> {
        &self.peer_handle
    }

    pub fn network_handle(&self) -> &NetworkHandle {
        &self.network_handle
    }

    pub fn connect_to_peer(&self, id: PeerId, addr: SocketAddr) {
        self.network_handle.add_peer(id, addr);
    }

    pub fn socket_addr(&self) -> SocketAddr {
        self.network_handle.local_addr()
    }
}
