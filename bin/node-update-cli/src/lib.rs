pub mod cli;
use clap::Parser;
use exe_runners::TaskExecutor;

pub fn run(task_executor: TaskExecutor) -> eyre::Result<()> {
    let args = cli::NodeUpdaterCli::parse();
    reth::CliRunner::try_default_runtime()
        .unwrap()
        .run_command_until_exit(|ctx| args.run(ctx.task_executor))
}
