use angstrom_types::orders::{OrderValidationOutcome, ValidatedOrder};
use order_pool::OrderPoolHandle;
use rand::{thread_rng, Rng};
use testing_tools::{
    mocks::{
        eth_events::MockEthEventHandle, network_events::MockNetworkHandle, validator::MockValidator
    },
    order_pool::TestnetOrderPool,
    type_generator::orders::{generate_rand_valid_limit_order, generate_random_valid_order}
};
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_order_indexing() {
    reth_tracing::init_test_tracing();

    let validator = MockValidator::default();
    let (mock_handle, network_handle, network_rx, order_rx) = MockNetworkHandle::new();
    let (mock_eth, eth_events) = MockEthEventHandle::new();
    let mut rng = thread_rng();

    let orders = (0..rng.gen_range(3..10))
        .map(|_| generate_rand_valid_limit_order())
        .collect::<Vec<_>>();

    let orderpool = TestnetOrderPool::new_full_mock(
        validator,
        network_handle,
        eth_events,
        order_rx,
        network_rx
    );

    for order in &orders {
        let signer = order.recover_signer().unwrap();
        ValidatedOrder::
        let validation_outcome = OrderValidationOutcome::Valid { order: (), propagate: false };
        validator.add_limit_order(signer, order.clone());
    }

    for order in orders {
        orderpool
            .pool_handle
            .new_limit_order(angstrom_types::orders::OrderOrigin::External, order)
    }
}
