/*
use std::time::Duration;

use reth_provider::test_utils::NoopProvider;


#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_startup() {
    reth_tracing::init_test_tracing();
    let noop = NoopProvider::default();
    let mut testnet = AngstromTestnet::new(4, noop).await;

    let res = tokio::time::timeout(Duration::from_secs(1), testnet.connect_all_peers()).await;
    assert!(res.is_ok(), "failed to connect all peers within 1 seconds");
}


*/
