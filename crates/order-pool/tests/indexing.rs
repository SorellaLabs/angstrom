use std::time::Duration;

use angstrom_types::orders::{OrderPriorityData, OrderValidationOutcome, ValidatedOrder};
use order_pool::{OrderPoolHandle, PoolConfig};
use rand::{thread_rng, Rng};
use testing_tools::{
    mocks::{
        eth_events::MockEthEventHandle, network_events::MockNetworkHandle, validator::MockValidator
    },
    order_pool::TestnetOrderPool,
    type_generator::orders::generate_rand_valid_limit_order
};
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_order_indexing() {
    reth_tracing::init_test_tracing();

    let validator = MockValidator::default();
    let (_, network_handle, network_rx, order_rx) = MockNetworkHandle::new();
    let (_, eth_events) = MockEthEventHandle::new();
    let mut rng = thread_rng();

    let orders = (0..rng.gen_range(3..5))
        .map(|_| generate_rand_valid_limit_order())
        .collect::<Vec<_>>();

    let mut pool_config = PoolConfig::default();
    pool_config.ids = vec![0, 1, 2, 3, 4, 5];

    let mut orderpool = TestnetOrderPool::new_full_mock(
        validator.clone(),
        pool_config,
        network_handle,
        eth_events,
        order_rx,
        network_rx
    );

    for order in &orders {
        let signer = order.recover_signer().unwrap();
        let order = order.clone().try_into().unwrap();

        let validated = ValidatedOrder {
            order,
            data: OrderPriorityData { gas: 69420, price: 12678, volume: 23123 },
            is_bid: rng.gen(),
            pool_id: rng.gen_range(0..=5),
            location: angstrom_types::orders::OrderLocation::LimitPending
        };

        let validation_outcome =
            OrderValidationOutcome::Valid { order: validated, propagate: false };

        validator.add_limit_order(signer, validation_outcome);
    }

    let order_count = orders.len();
    for order in orders {
        orderpool
            .pool_handle
            .new_limit_order(angstrom_types::orders::OrderOrigin::External, order)
    }

    let mut new_orders = orderpool.pool_handle.subscribe_new_orders();
    let mut have = 0;

    let res = tokio::time::timeout(
        Duration::from_secs(2),
        orderpool.poll_until(|| {
            if let Ok(_) = new_orders.as_mut().try_recv() {
                have += 1;
            }
            order_count == have
        })
    )
    .await;

    assert_eq!(res, Ok(true), "orderpool failed to index new orders");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_pool_eviction() {
    reth_tracing::init_test_tracing();

    let validator = MockValidator::default();
    let (_, network_handle, network_rx, order_rx) = MockNetworkHandle::new();
    let (eth_handle, eth_events) = MockEthEventHandle::new();
    let mut rng = thread_rng();

    let orders = (0..rng.gen_range(3..5))
        .map(|_| generate_rand_valid_limit_order())
        .collect::<Vec<_>>();

    let mut pool_config = PoolConfig::default();
    pool_config.ids = vec![0, 1, 2, 3, 4, 5];

    let mut orderpool = TestnetOrderPool::new_full_mock(
        validator.clone(),
        pool_config,
        network_handle,
        eth_events,
        order_rx,
        network_rx
    );

    for order in &orders {
        let signer = order.recover_signer().unwrap();
        let order = order.clone().try_into().unwrap();

        let validated = ValidatedOrder {
            order,
            data: OrderPriorityData { gas: 69420, price: 12678, volume: 23123 },
            is_bid: rng.gen(),
            pool_id: rng.gen_range(0..=5),
            location: angstrom_types::orders::OrderLocation::LimitPending
        };

        let validation_outcome =
            OrderValidationOutcome::Valid { order: validated, propagate: false };

        validator.add_limit_order(signer, validation_outcome);
    }

    let to_evict = orders
        .iter()
        .take(1)
        .map(|o| o.recover_signer().unwrap())
        .collect::<Vec<_>>()
        .remove(0);

    let order_count = orders.len();
    for order in orders {
        orderpool
            .pool_handle
            .new_limit_order(angstrom_types::orders::OrderOrigin::External, order)
    }

    let mut new_orders = orderpool.pool_handle.subscribe_new_orders();
    let mut have = 0;

    let res = tokio::time::timeout(
        Duration::from_secs(2),
        orderpool.poll_until(|| {
            if let Ok(_) = new_orders.as_mut().try_recv() {
                have += 1;
            }
            order_count == have
        })
    )
    .await;
    assert_eq!(res, Ok(true), "orderpool failed to index new orders");

    // send order for re validation. this should fail and we should have one less
    // order in the pool
    eth_handle.state_changes(vec![to_evict]);

    // progress the pool for one second.
    let _ = tokio::time::timeout(Duration::from_secs(1), orderpool.poll_until(|| false)).await;

    let orders = orderpool.pool_handle.clone();
    let orders = orders.get_all_vanilla_orders();
    let (orders, _) = futures::join!(
        orders,
        tokio::time::timeout(Duration::from_secs(1), orderpool.poll_until(|| false))
    );

    assert_eq!(orders.limit.len(), order_count - 1, "failed to evict stale order");
}
