use std::pin::Pin;

use angstrom_types::testnet::InitialTestnetState;
use futures::Future;
use op_alloy_network::Optimism;
use op_testing_tools::{
    agents::AgentConfig, controllers::enviroments::OpAngstromTestnet,
    types::config::OpTestnetConfig, utils::noop_agent
};
use reth_optimism_primitives::OpPrimitives;
use reth_provider::test_utils::NoopProvider;
use reth_tasks::TaskExecutor;

use crate::cli::{init_tracing, testnet::TestnetCli};

pub(crate) async fn run_testnet(executor: TaskExecutor, cli: TestnetCli) -> eyre::Result<()> {
    let config = cli.make_config()?;

    let testnet = OpAngstromTestnet::spawn_testnet(
        NoopProvider::default(),
        config,
        vec![noop_agent],
        executor.clone()
    )
    .await?;

    testnet.run_to_completion().await;
    Ok(())
}
