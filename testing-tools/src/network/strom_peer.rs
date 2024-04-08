use std::{collections::HashSet, sync::Arc, task::Poll};

use angstrom_network::{StromNetworkHandle, StromNetworkManager};
use futures::{Future, FutureExt};
use parking_lot::RwLock;
use reth_network::test_utils::Peer;
use reth_primitives::Address;
use reth_provider::{test_utils::NoopProvider, BlockReader, HeaderProvider};
use reth_rpc_types::{pk_to_id, PeerId};
use reth_transaction_pool::{test_utils::TestPool, TransactionPool};
use secp256k1::{PublicKey, Secp256k1};

use crate::network::SecretKey;

pub struct StromPeer<C = NoopProvider, Pool = TestPool> {
    /// the default ethereum network peer
    pub eth_peer:      Peer<C, Pool>,
    // strom extensions
    pub strom_network: StromNetworkManager<C>,
    pub secret_key:    SecretKey
}

impl<C: Unpin, Pool> StromPeer<C, Pool> {
    pub fn get_node_public_key(&self) -> PeerId {
        let pub_key = PublicKey::from_secret_key(&Secp256k1::default(), &self.secret_key);
        pk_to_id(&pub_key)
    }

    pub fn disconnect_peer(&self, id: PeerId) {
        self.get_handle().remove_peer(id)
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

    fn get_validator_set(&self) -> Arc<RwLock<HashSet<Address>>> {
        self.strom_network.swarm().state().validators()
    }

    pub fn get_handle(&self) -> StromNetworkHandle {
        self.strom_network.get_handle()
    }
}

impl<C, Pool> Future for StromPeer<C, Pool>
where
    C: BlockReader + HeaderProvider + Unpin,
    Pool: TransactionPool + Unpin + 'static
{
    type Output = ();

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>
    ) -> std::task::Poll<Self::Output> {
        let this = self.get_mut();
        if let Poll::Ready(_) = this.eth_peer.poll_unpin(cx) {
            return Poll::Ready(())
        }
        if let Poll::Ready(_) = this.strom_network.poll_unpin(cx) {
            return Poll::Ready(())
        }

        Poll::Pending
    }
}
