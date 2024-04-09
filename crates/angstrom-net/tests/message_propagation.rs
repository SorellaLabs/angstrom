use std::time::Duration;

use reth_provider::test_utils::NoopProvider;
use testing_tools::{network::AngstromTestnet, type_generator::orders::generate_valid_order};

#[tokio::test(flavor = "multi_thread", worker_threads = 5)]
async fn test_broadcast_order_propagation() {
    reth_tracing::init_test_tracing();
    let noop = NoopProvider::default();
    let mut testnet = AngstromTestnet::new(3, noop).await;

    // connect all peers
    let res = tokio::time::timeout(Duration::from_secs(3), testnet.connect_all_peers()).await;
    assert!(res.is_ok(), "failed to connect all peers within 3 seconds");

    let orders = (0..3).map(|_| generate_valid_order()).collect::<Vec<_>>();

    let res = tokio::time::timeout(
        Duration::from_secs(4),
        testnet.broadcast_message_orders(angstrom_network::StromMessage::PropagatePooledOrders(
            orders.clone()
        ))
    )
    .await;

    assert_eq!(res, Ok(true), "failed to receive and react to order within 4 seconds");

    let res = tokio::time::timeout(
        Duration::from_secs(4),
        testnet.broadcast_message_orders(angstrom_network::StromMessage::PropagatePooledOrders(
            orders
        ))
    )
    .await;

    assert_eq!(res, Ok(true), "failed to receive and react to order within 4 seconds");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 10)]
async fn test_singular_order_propagation() {
    reth_tracing::init_test_tracing();
    let noop = NoopProvider::default();
    let mut testnet = AngstromTestnet::new(3, noop).await;

    // connect all peers
    let res = tokio::time::timeout(Duration::from_secs(3), testnet.connect_all_peers()).await;
    assert!(res.is_ok(), "failed to connect all peers within 3 seconds");

    let orders = (0..3).map(|_| generate_valid_order()).collect::<Vec<_>>();

    let res = tokio::time::timeout(
        Duration::from_secs(4),
        testnet.send_order_message(angstrom_network::StromMessage::PropagatePooledOrders(
            orders.clone()
        ))
    )
    .await;

    assert_eq!(res, Ok(true), "failed to receive and react to order within 4 seconds");

    let res = tokio::time::timeout(
        Duration::from_secs(4),
        testnet.send_order_message(angstrom_network::StromMessage::PropagatePooledOrders(
            orders.clone()
        ))
    )
    .await;

    assert_eq!(res, Ok(true), "failed to receive and react to order within 4 seconds");
}
