use std::time::Duration;

use reth_provider::test_utils::NoopProvider;
use testing_tools::{
    network::AngstromTestnet,
    type_generator::{
        consensus::{generate_random_commit, generate_random_preposal, generate_random_proposal},
        orders::generate_random_valid_order
    }
};

#[tokio::test(flavor = "multi_thread", worker_threads = 5)]
async fn test_broadcast_order_propagation() {
    reth_tracing::init_test_tracing();
    let noop = NoopProvider::default();
    let mut testnet = AngstromTestnet::new(3, noop).await;

    // connect all peers
    let res = tokio::time::timeout(Duration::from_secs(3), testnet.connect_all_peers()).await;
    assert!(res.is_ok(), "failed to connect all peers within 3 seconds");

    let orders = (0..3)
        .map(|_| generate_random_valid_order())
        .collect::<Vec<_>>();

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

#[tokio::test(flavor = "multi_thread", worker_threads = 5)]
async fn test_singular_order_propagation() {
    reth_tracing::init_test_tracing();
    let noop = NoopProvider::default();
    let mut testnet = AngstromTestnet::new(3, noop).await;

    // connect all peers
    //
    let res = tokio::time::timeout(Duration::from_secs(3), testnet.connect_all_peers()).await;
    assert!(res.is_ok(), "failed to connect all peers within 3 seconds");

    let orders = (0..3)
        .map(|_| generate_random_valid_order())
        .collect::<Vec<_>>();

    let delay_seconds = 6;

    let res = tokio::time::timeout(
        Duration::from_secs(delay_seconds),
        testnet.send_order_message(angstrom_network::StromMessage::PropagatePooledOrders(
            orders.clone()
        ))
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
        testnet.send_order_message(angstrom_network::StromMessage::PropagatePooledOrders(
            orders.clone()
        ))
    )
    .await;

    assert_eq!(res, Ok(true), "failed to receive and react to order within 4 seconds");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 5)]
async fn test_broadcast_consensus_propagation() {
    reth_tracing::init_test_tracing();
    let noop = NoopProvider::default();
    let mut testnet = AngstromTestnet::new(4, noop).await;

    // connect all peers
    let res = tokio::time::timeout(Duration::from_secs(3), testnet.connect_all_peers()).await;
    assert!(res.is_ok(), "failed to connect all peers within 3 seconds");

    for _ in 0..3 {
        // commits
        let commit = generate_random_commit();
        let delay_seconds = 3;
        let res = tokio::time::timeout(
            Duration::from_secs(delay_seconds),
            testnet.send_consensus_broadcast(angstrom_network::StromMessage::Commit(commit))
        )
        .await;
        assert_eq!(
            res,
            Ok(true),
            "failed to receive and react to commit within {} second{}",
            delay_seconds,
            if delay_seconds == 1 { "" } else { "s" }
        );

        // preposals
        let preposal = generate_random_preposal();
        let res = tokio::time::timeout(
            Duration::from_secs(1),
            testnet.send_consensus_broadcast(angstrom_network::StromMessage::PrePropose(preposal))
        )
        .await;
        assert_eq!(res, Ok(true), "failed to receive and react to preposal within 1 second");

        // proposals
        let proposal = generate_random_proposal();
        let res = tokio::time::timeout(
            Duration::from_secs(1),
            testnet.send_consensus_broadcast(angstrom_network::StromMessage::Propose(proposal))
        )
        .await;
        assert_eq!(res, Ok(true), "failed to receive and react to proposal within 1 second");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 5)]
async fn test_consensus_propagation() {
    reth_tracing::init_test_tracing();
    let noop = NoopProvider::default();
    let mut testnet = AngstromTestnet::new(4, noop).await;

    // connect all peers
    let res = tokio::time::timeout(Duration::from_secs(3), testnet.connect_all_peers()).await;
    assert!(res.is_ok(), "failed to connect all peers within 3 seconds");

    for _ in 0..3 {
        // commits
        let commit = generate_random_commit();
        let res = tokio::time::timeout(
            Duration::from_secs(1),
            testnet.send_consensus_message(angstrom_network::StromMessage::Commit(commit))
        )
        .await;
        assert_eq!(res, Ok(true), "failed to receive and react to commit within 1 second");

        // preposals
        let preposal = generate_random_preposal();
        let res = tokio::time::timeout(
            Duration::from_secs(1),
            testnet.send_consensus_message(angstrom_network::StromMessage::PrePropose(preposal))
        )
        .await;
        assert_eq!(res, Ok(true), "failed to receive and react to preposal within 1 second");

        // proposals
        let proposal = generate_random_proposal();
        let res = tokio::time::timeout(
            Duration::from_secs(1),
            testnet.send_consensus_message(angstrom_network::StromMessage::Propose(proposal))
        )
        .await;
        assert_eq!(res, Ok(true), "failed to receive and react to proposal within 1 second");
    }
}
