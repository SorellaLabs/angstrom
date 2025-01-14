use std::pin::Pin;

use angstrom_types::testnet::InitialTestnetState;
use futures::Future;
use reth_provider::test_utils::NoopProvider;
use reth_tasks::TaskExecutor;
use testing_tools::{
    agents::AgentConfig, controllers::enviroments::AngstromTestnet, types::config::TestnetConfig
};

use crate::cli::testnet::TestnetCli;

pub(crate) async fn run_testnet(executor: TaskExecutor, cli: TestnetCli) -> eyre::Result<()> {
    let config = cli.make_config()?;

    let testnet = AngstromTestnet::spawn_testnet(NoopProvider::default(), config, vec![a]).await?;

    testnet.run_to_completion(executor).await;
    Ok(())
}

fn a<'a>(
    _: &'a InitialTestnetState,
    _: AgentConfig
) -> Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + 'a>> {
    Box::pin(async { eyre::Ok(()) })
}
