#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_validation_pass() {
    reth_tracing::init_test_tracing();
}
