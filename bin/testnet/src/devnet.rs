use reth_provider::test_utils::NoopProvider;
use reth_tasks::TaskExecutor;
use testing_tools::{
    controllers::enviroments::AngstromTestnet,
    types::{
        actions::WithAction, checked_actions::WithCheckedAction, checks::WithCheck,
        config::DevnetConfig
    }
};
use tracing::{debug, info};

use crate::cli::devnet::DevnetCli;

pub(crate) async fn run_devnet(executor: TaskExecutor, cli: DevnetCli) -> eyre::Result<()> {
    let config = cli.make_config();
    let mut testnet = AngstromTestnet::spawn_devnet(NoopProvider::default(), config)
        .await?
        .as_state_machine();

    info!("deployed state machine");

    testnet.check_block(15);
    testnet.advance_block();
    testnet.check_block(16);
    testnet.send_pooled_orders(vec![]);
    debug!("added pooled orders to state machine");

    testnet.run().await;

    Ok(())
}
