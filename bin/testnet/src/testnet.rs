use std::pin::Pin;

use angstrom_types::testnet::InitialTestnetState;
use futures::Future;
use reth_provider::test_utils::NoopProvider;
use reth_tasks::TaskExecutor;
use testing_tools::{
    agents::AgentConfig, controllers::enviroments::AngstromTestnet, types::config::TestnetConfig,
    utils::noop_agent
};

use crate::cli::{init_tracing, testnet::TestnetCli};

pub(crate) async fn run_testnet(executor: TaskExecutor, cli: TestnetCli) -> eyre::Result<()> {
    let config = cli.make_config()?;

    let testnet =
        AngstromTestnet::spawn_testnet(NoopProvider::default(), config, vec![noop_agent]).await?;

    testnet.run_to_completion(executor).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn testnet_deploy() {
        init_tracing(4);
        let cli = TestnetCli {
            eth_fork_url: "wss://ethereum-rpc.publicnode.com".to_string(),
            ..Default::default()
        };

        let testnet = AngstromTestnet::spawn_testnet(
            NoopProvider::default(),
            cli.make_config().unwrap(),
            vec![noop_agent]
        )
        .await;
        assert!(testnet.is_ok());
    }
}
