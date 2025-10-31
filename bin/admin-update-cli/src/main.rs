use clap::Parser;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> eyre::Result<()> {
    let args = admin_update_cli::cli::NodeUpdaterCli::parse();

    args.run().await
}
