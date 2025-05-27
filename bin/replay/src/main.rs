use alloy_provider::ext::AnvilApi;
use futures::FutureExt;
use telemetry::blocklog::BlockLog;
use testing_tools::{
    controllers::enviroments::replay::replay_stuff,
    providers::{AnvilInitializer, AnvilProvider, AnvilStateProvider},
    types::{
        WithWalletProvider,
        config::{ReplayConfig, TestingNodeConfig}
    }
};
const LOG_STR: &str = "";

fn main() {
    let replay_log = BlockLog::from_deflate_base64_str(LOG_STR);
    let config = ReplayConfig::new(true, "Test URL".to_string());
    let node_config = TestingNodeConfig::new(1, config.clone(), 10);
    reth::CliRunner::try_default_runtime()
        .unwrap()
        .run_command_until_exit(async move |ctx| -> eyre::Result<()> {
            let ex = ctx.task_executor;
            let provider = if config.testnet_replay() {
                // If we're replaying from testnet, we want to do local initialization
                let (mut init, instance, deployed) =
                    AnvilInitializer::new(node_config.clone(), vec![]).await?;
                init.deploy_pool_fulls(vec![]).await?;
                init.rpc_provider().anvil_mine(Some(10), None).await?;
                AnvilProvider::new(AnvilStateProvider::new(init), instance, Some(deployed))
                    .into_state_provider()
            } else {
                // Otherwise we want to just fork the current chain as specified on the command
                // line
                AnvilProvider::from_future(
                    node_config
                        .spawn_anvil_rpc()
                        .then(async |x| x.map(|y| (y.0, y.1, None))),
                    false
                )
                .await?
            };

            // Deploy things if we don't already have a deployment?

            // Get the block
            // Replay our BlockLog
            replay_stuff(provider, ex, replay_log).await?;
            Ok(())
        })
        .unwrap();
}
