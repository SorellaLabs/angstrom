#[tokio::main]
async fn main() -> eyre::Result<()> {
    protocol_fees::run().await
}
