use angstrom_types::primitive::AngstromAddressConfig;
use op_testing_tools::{
    controllers::enviroments::OpAngstromTestnet,
    types::{actions::WithAction, checks::WithCheck, config::DevnetConfig}
};
use reth_tasks::TaskExecutor;
use tracing::{debug, info};

use crate::{cli::devnet::DevnetCli, simulations::token_prices_update_new_pools};

pub(crate) async fn run_devnet(executor: TaskExecutor, cli: DevnetCli) -> eyre::Result<()> {
    token_prices_update_new_pools::run_devnet(executor, cli).await?;

    Ok(())
}

async fn basic_example(executor: TaskExecutor, cli: DevnetCli) -> eyre::Result<()> {
    let config = cli.make_config()?;

    AngstromAddressConfig::BASE_TESTNET.init();
    let mut testnet = OpAngstromTestnet::spawn_devnet(config, executor.clone())
        .await?
        .as_state_machine();

    info!("deployed state machine");

    testnet.check_block(15);
    testnet.advance_block();
    testnet.check_block(16);
    // TODO(mempirate): Add orders
    debug!("added pooled orders to state machine");

    testnet.run().await;

    Ok(())
}
