use testing_tools::mocks::eth_events::MockEthEventHandle;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn test_validation_pass() {
    reth_tracing::init_test_tracing();
    let (handle, eth_events) = MockEthEventHandle::new();
}
