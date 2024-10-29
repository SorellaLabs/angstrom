use std::time::Duration;

use angstrom_network::StromMessage;
use reth_provider::test_utils::NoopProvider;
use testing_tools::testnet_controllers::AngstromTestnet;
use testnet::cli::Cli;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> eyre::Result<()> {
    let config = Cli::build_config();

    let network_controller =
        AngstromTestnet::spawn_testnet(NoopProvider::default(), config).await?;

    send_bundles(network_controller).await?;

    Ok(())
}

#[allow(dead_code)]
async fn do_thing(network_controller: AngstromTestnet<NoopProvider>) -> eyre::Result<()> {
    loop {
        tokio::time::sleep(Duration::from_secs(11)).await;
        network_controller
            .run_event(None, |peer| async { peer.send_bundles_to_network(peer.peer_id(), 10) })
            .await?;
        // Ok(())
    }
}

async fn send_bundles(mut network_controller: AngstromTestnet<NoopProvider>) -> eyre::Result<()> {
    loop {
        tokio::time::sleep(Duration::from_secs(11)).await;
        let orders = vec![];
        let passed = network_controller
            .broadcast_orders_message(
                Some(0),
                StromMessage::PropagatePooledOrders(orders.clone()),
                orders
            )
            .await;

        assert!(passed);

        // Ok(())
    }
}
