//! CLI definition and entrypoint to executable
#![allow(unused)]
pub mod cli;
pub mod simulations;
mod testnet;
pub(crate) use testnet::run_testnet;

pub fn run() -> eyre::Result<()> {
    reth::CliRunner::try_default_runtime()
        .unwrap()
        .run_command_until_exit(|ctx| cli::OpAngstromTestnetCli::run_all(ctx.task_executor))
}
