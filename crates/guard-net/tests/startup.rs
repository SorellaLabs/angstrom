use std::{collections::HashSet, sync::Arc};

use guard_network::{NetworkBuilder, StatusState, VerificationSidecar};
use parking_lot::RwLock;
use rand::thread_rng;
use reth_network::test_utils::{PeerConfig, Testnet};
use reth_primitives::*;
use reth_provider::test_utils::NoopProvider;
use reth_tasks::TokioTaskExecutor;
use reth_transaction_pool::test_utils::TestPool;
use secp256k1::{Secp256k1, SecretKey};
use validation::init_validation;

#[tokio::test]
async fn test_startup() {
    reth_tracing::init_test_tracing();

    let tp = TokioTaskExecutor::default();
    let mut rng = thread_rng();
    let init = (0..3)
        .into_iter()
        .map(|_| SecretKey::new(&mut rng))
        .map(|key| (key.clone(), PeerConfig::with_secret_key(NoopProvider::default(), key)))
        .collect::<Vec<_>>();

    let mut validators: HashSet<Address> = HashSet::default();

    for (secret_key, _) in &init {
        let secp = Secp256k1::default();
        let pub_key = secret_key.public_key(&secp);
        let addr = reth_primitives::public_key_to_address(pub_key);
        validators.insert(addr);
    }
    let validator_set = Arc::new(RwLock::new(validators));

    let net: Testnet<NoopProvider, TestPool> = Testnet::default();

    for (secret_key, peer) in init {
        let state = StatusState {
            version:   0,
            chain:     Chain::mainnet(),
            peer:      PeerId::default(),
            timestamp: 0
        };

        let verification =
            VerificationSidecar { status: state, has_sent: false, has_received: false, secret_key };

        let validator =
            init_validation(NoopProvider::default(), Box::pin(futures::stream::empty()));

        let (pool_tx, pool_rx) =
            reth_metrics::common::mpsc::metered_unbounded_channel("order pool");

        let (consensus_tx, consensus_rx) =
            reth_metrics::common::mpsc::metered_unbounded_channel("consensus");

        let (protocol, network_handle) = NetworkBuilder::new(NoopProvider::default(), verification)
            .with_pool_manager(pool_tx)
            .with_consensus_manager(consensus_tx)
            .with_validator_set(validator_set.clone())
            .build(tp.clone());

        let peer_launched = peer.launch().await.unwrap();
        // let network = peer_launched.network_mut();
        // network.add_rlpx_sub_protocol(protocol);
    }
}
