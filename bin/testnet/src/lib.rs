//! CLI definition and entrypoint to executable
pub mod config;
pub(crate) mod utils;

use clap::Parser;
use config::AngstromTestnetCli;
use reth::{tasks::TaskExecutor, CliRunner};
use reth_provider::test_utils::NoopProvider;
use secp256k1::Secp256k1;
use testing_tools::controllers::testnet::{AngstromTestnet, TestnetConfig};

pub fn run() -> eyre::Result<()> {
    CliRunner::default().run_command_until_exit(|ctx| execute(ctx.task_executor))
}

async fn execute(executor: TaskExecutor) -> eyre::Result<()> {
    let cli = AngstromTestnetCli::parse();
    executor.spawn_critical("metrics", cli.clone().init_metrics());

    let testnet_config = cli.load_config()?;
    let my_node_config = testnet_config.my_node_config()?;

    let pub_key = my_node_config.secret_key.public_key(&Secp256k1::default());
    let initial_validators = testnet_config.initial_validators();

    let iam_leader = my_node_config.is_leader;

    let config = TestnetConfig::new(
        7,
        2,
        testnet_config.leader_ws_url()?,
        my_node_config.address,
        pub_key,
        my_node_config.signing_key,
        my_node_config.secret_key,
        testnet_config.pools_keys,
        testnet_config.angstrom_address,
        testnet_config.pool_manager_address,
        iam_leader.then_some("ws://35.245.117.24:8546"),
        iam_leader
    );

    let testnet =
        AngstromTestnet::spawn_testnet(NoopProvider::default(), config, initial_validators).await?;

    executor
        .spawn_critical_blocking("testnet", testnet.run())
        .await?;

    Ok(())
}
