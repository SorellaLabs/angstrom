use reth_provider::test_utils::NoopProvider;
use testing_tools::network::AngstromTestnet;

#[tokio::test(flavor = "multi_thread")]
async fn test_startup() {
    reth_tracing::init_test_tracing();
    let noop = NoopProvider::default();
    tracing::info!("starting testnet");
    let mut testnet = AngstromTestnet::new(3, noop).await;

    tracing::info!("all peers started. connecting all of them");
    testnet.connect_all_peers().await;
    tracing::info!("shit connected");
}
