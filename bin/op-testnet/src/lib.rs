//! CLI definition and entrypoint to executable
#![allow(unused)]
pub mod cli;
mod op_devnet;
mod op_testnet;
pub mod simulations;
pub(crate) use op_devnet::run_devnet;
pub(crate) use op_testnet::run_testnet;

pub fn run() -> eyre::Result<()> {
    reth::CliRunner::try_default_runtime()
        .unwrap()
        .run_command_until_exit(|ctx| cli::OpAngstromTestnetCli::run_all(ctx.task_executor))
}
