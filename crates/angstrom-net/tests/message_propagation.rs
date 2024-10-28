use std::time::Duration;

use angstrom_network::StromMessage;
use reth_provider::test_utils::NoopProvider;
use testing_tools::testnet_controllers::{AngstromTestnet, AngstromTestnetConfig, TestnetKind};

#[tokio::test(flavor = "multi_thread", worker_threads = 5)]
#[serial_test::serial]
async fn test_broadcast_order_propagation() {
    reth_tracing::init_test_tracing();
    let config = AngstromTestnetConfig {
        intial_node_count:       3,
        initial_rpc_port:        5000,
        testnet_block_time_secs: 12,
        testnet_kind:            TestnetKind::new_raw()
    };
    let mut testnet = AngstromTestnet::spawn_testnet(NoopProvider::default(), config)
        .await
        .unwrap();

    // let orders = (0..3)
    //     .map(|_| generate_random_valid_order())
    //     .collect::<Vec<_>>();
    let orders = vec![];

    let delay_seconds = 4;
    let res = tokio::time::timeout(
        Duration::from_secs(delay_seconds),
        testnet.broadcast_orders_message(
            Some(0),
            StromMessage::PropagatePooledOrders(orders.clone()),
            orders.clone()
        )
    )
    .await;

    assert_eq!(
        res,
        Ok(true),
        "failed to receive and react to order within {} seconds",
        delay_seconds
    );

    let res = tokio::time::timeout(
        Duration::from_secs(delay_seconds),
        testnet.broadcast_orders_message(
            Some(0),
            StromMessage::PropagatePooledOrders(orders.clone()),
            orders
        )
    )
    .await;

    assert_eq!(res, Ok(true), "failed to receive and react to order within 4 seconds");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 5)]
#[serial_test::serial]
async fn test_singular_order_propagation() {
    reth_tracing::init_test_tracing();
    let config = AngstromTestnetConfig {
        intial_node_count:       3,
        initial_rpc_port:        5000,
        testnet_block_time_secs: 12,
        testnet_kind:            TestnetKind::new_raw()
    };

    // connect all peers
    //
    let testnet = tokio::time::timeout(
        Duration::from_secs(30),
        AngstromTestnet::spawn_testnet(NoopProvider::default(), config)
    )
    .await;
    assert!(matches!(testnet, Ok(Ok(_))), "failed to connect all peers within 30 seconds");

    let mut testnet = testnet.unwrap().unwrap();

    // let orders = (0..3)
    //     .map(|_| generate_random_valid_order())
    //     .collect::<Vec<_>>();
    let orders = vec![];

    let delay_seconds = 8;

    let res = tokio::time::timeout(
        Duration::from_secs(delay_seconds),
        testnet.broadcast_orders_message(
            Some(0),
            StromMessage::PropagatePooledOrders(orders.clone()),
            orders.clone()
        )
    )
    .await;

    assert_eq!(
        res,
        Ok(true),
        "failed to receive and react to order within {} seconds",
        delay_seconds
    );

    let res = tokio::time::timeout(
        Duration::from_secs(4),
        testnet.broadcast_orders_message(
            Some(0),
            StromMessage::PropagatePooledOrders(orders.clone()),
            orders.clone()
        )
    )
    .await;

    assert_eq!(res, Ok(true), "failed to receive and react to order within 4 seconds");
}
