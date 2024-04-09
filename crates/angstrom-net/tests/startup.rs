use reth_provider::test_utils::NoopProvider;
use testing_tools::network::AngstromTestnet;

#[tokio::test(flavor = "multi_thread")]
async fn test_startup() {
    reth_tracing::init_test_tracing();
    let noop = NoopProvider::default();
    let mut testnet = AngstromTestnet::new(3, noop).await;

    testnet.connect_all_peers().await;
    tracing::info!("shit connected");
}
