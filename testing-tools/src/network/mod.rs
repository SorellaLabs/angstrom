use std::{collections::HashSet, sync::Arc};
mod strom_peer;

use angstrom_network::{NetworkBuilder, StatusState, VerificationSidecar};
use parking_lot::RwLock;
use rand::thread_rng;
use reth_network::test_utils::{PeerConfig, Testnet};
use reth_primitives::*;
use reth_provider::test_utils::NoopProvider;
use reth_rpc_types::pk_to_id;
use reth_tasks::TokioTaskExecutor;
use reth_transaction_pool::test_utils::TestPool;
use secp256k1::{Secp256k1, SecretKey};
use validation::init_validation;

use self::strom_peer::StromPeer;

/// the goal of the angstrom testnet is to extend reth's baseline tests
/// as-well as expand appon to allow for composing tests and ensuring full
/// performance
pub struct AngstromTestnet {
    pub peers: Vec<StromPeer>
}

impl AngstromTestnet {
    pub fn new(peers: usize) -> Self {
        Self { peers: vec![] }
    }

    pub fn add_new_peer(&mut self, peer: StromPeer) {
        self.peers.push(peer)
    }
}
