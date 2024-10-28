use std::{collections::HashSet, sync::Arc};

use alloy_primitives::Address;
use angstrom_network::{StromNetworkEvent, StromNetworkHandle, StromNetworkManager};
use angstrom_types::primitive::PeerId;
use parking_lot::RwLock;
use tokio_stream::wrappers::UnboundedReceiverStream;

#[derive(Clone)]
pub struct StromNetworkPeer {
    network_handle: StromNetworkHandle,
    validator_set:  Arc<RwLock<HashSet<Address>>>
}

impl StromNetworkPeer {
    pub fn new<C: Unpin>(strom_network: &StromNetworkManager<C>) -> Self {
        Self {
            network_handle: strom_network.get_handle(),
            validator_set:  strom_network.swarm().state().validators().clone()
        }
    }

    pub fn network_handle(&self) -> &StromNetworkHandle {
        &self.network_handle
    }

    pub fn validator_set(&self) -> Arc<RwLock<HashSet<Address>>> {
        self.validator_set.clone()
    }

    pub fn disconnect_peer(&self, id: PeerId) {
        self.network_handle.remove_peer(id)
    }

    pub fn peer_count(&self) -> usize {
        self.network_handle.peer_count()
    }

    pub fn remove_validator(&self, id: PeerId) {
        let addr = Address::from_raw_public_key(id.as_slice());
        let set = self.validator_set();
        set.write().remove(&addr);
    }

    pub fn add_validator(&self, id: PeerId) {
        let addr = Address::from_raw_public_key(id.as_slice());
        let set = self.validator_set();
        set.write().insert(addr);
    }

    pub fn subscribe_network_events(&self) -> UnboundedReceiverStream<StromNetworkEvent> {
        self.network_handle.subscribe_network_events()
    }
}
