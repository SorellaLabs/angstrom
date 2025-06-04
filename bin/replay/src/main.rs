use clap::Parser;
use replay::{ReplayCli, init_tracing};
use reth_provider::noop::NoopProvider;
use telemetry::blocklog::BlockLog;
use testing_tools::{controllers::enviroments::AngstromTestnet, types::config::ReplayConfig};

const LOG_DATA: &str = "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";

fn read_log_from_stdin() -> BlockLog {
    // let mut buf = String::new();
    // let mut handle = stdin().lock();
    // handle.read_to_string(&mut buf).unwrap();
    BlockLog::from_deflate_base64_str(LOG_DATA)
}

fn main() {
    init_tracing(2);
    let cli = ReplayCli::parse();
    let raw_log = read_log_from_stdin();
    let target_block = raw_log.blocknum().saturating_sub(83);
    tracing::info!(target_block, log_block = raw_log.blocknum(), "Loaded and registered log");
    let initial_state = cli.load_pool_keys().unwrap();
    let config = ReplayConfig::new(
        initial_state,
        cli.testnet_replay,
        cli.eth_fork_url.clone(),
        target_block
    );
    // let node_config = TestingNodeConfig::new(1, config.clone(), 10);
    reth::CliRunner::try_default_runtime()
        .unwrap()
        .run_command_until_exit(async move |ctx| -> eyre::Result<()> {
            // let ex = ctx.task_executor;
            // let (replay_log, provider, initial_state) =
            //     build_log_and_provider(node_config, raw_log, ex.clone()).await?;
            // // Replay our BlockLog
            // replay_stuff(provider, ex, replay_log, initial_state).await?;

            let (net, state_rx) = AngstromTestnet::spawn_replay(
                NoopProvider::mainnet(),
                config,
                ctx.task_executor.clone()
            )
            .await?;
            net.replay_stuff(ctx.task_executor, raw_log, state_rx)
                .await?;
            Ok(())
        })
        .unwrap();
}
