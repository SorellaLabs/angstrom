mod cli;
pub mod commands;
use clap::Parser;
use exe_runners::TaskExecutor;

pub async fn run(task_executor: TaskExecutor) -> eyre::Result<()> {
    let args = cli::NodeUpdaterCli::parse();

    args.run(task_executor).await
}
