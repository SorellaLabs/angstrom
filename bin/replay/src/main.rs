use clap::Parser;
use replay::{ReplayCli, init_tracing};
use telemetry::outputs::s3::S3Storage;
use testing_tools::replay::runner::ReplayRunner;

fn main() -> eyre::Result<()> {
    init_tracing(3);

    reth::CliRunner::try_default_runtime()
        .unwrap()
        .run_command_until_exit(async move |ctx| {
            let cli = ReplayCli::parse();
            let aws = S3Storage::new().await.unwrap();
            let rpc_port = 7000 + rand::random_range(0..1000);
            let snapshot = aws.retrieve_snapshot(&cli.id, cli.is_error).await?;
            let runner =
                ReplayRunner::new(snapshot, cli.eth_fork_url, rpc_port, ctx.task_executor.clone())
                    .await?;

            runner.run().await
        })
}
