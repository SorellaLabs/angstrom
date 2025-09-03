use angstrom_types::primitive::AngstromAddressConfig;
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

use crate::{cli::devnet::DevnetCli, simulations::token_prices_update_new_pools};

pub(crate) async fn run_devnet(executor: TaskExecutor, cli: DevnetCli) -> eyre::Result<()> {
    token_prices_update_new_pools::run_devnet(executor, cli).await?;

    Ok(())
}

async fn basic_example(executor: TaskExecutor, cli: DevnetCli) -> eyre::Result<()> {
    let config = cli.make_config()?;

    AngstromAddressConfig::INTERNAL_TESTNET.init();
    let mut testnet =
        AngstromTestnet::spawn_devnet(NoopProvider::default(), config, executor.clone())
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
