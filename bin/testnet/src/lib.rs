//! CLI definition and entrypoint to executable
#![allow(unused)]
pub mod cli;
mod devnet;
pub mod simulations;
mod testnet;
pub(crate) use devnet::run_devnet;
pub(crate) use testnet::run_testnet;

pub fn run() -> eyre::Result<()> {
    reth::CliRunner::default()
        .run_command_until_exit(|ctx| cli::AngstromTestnetCli::run_all(ctx.task_executor))
}
