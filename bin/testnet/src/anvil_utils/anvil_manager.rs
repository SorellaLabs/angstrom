use alloy::{node_bindings::Anvil, signers::local::PrivateKeySigner};
use alloy_node_bindings::AnvilInstance;
use alloy_provider::{builder, IpcConnect, ProviderBuilder, RootProvider};
use alloy_pubsub::PubSubFrontend;

pub async fn spawn_anvil(
    block_time: u64,
    fork_url: String
) -> eyre::Result<(AnvilInstance, RootProvider<PubSubFrontend>)> {
    let anvil = Anvil::new()
        .block_time(block_time)
        .fork_block_number(20214717)
        .fork(fork_url)
        .chain_id(1)
        .arg("--ipc")
        .try_spawn()?;

    let endpoint = "/tmp/anvil.ipc";
    tracing::info!(?endpoint);
    let ipc = IpcConnect::new(endpoint.to_string());
    let sk: PrivateKeySigner = anvil.keys()[0].clone().into();

    let rpc = ProviderBuilder::new()
        .with_recommended_fillers()
        .wallet(sk)
        .on_ipc(ipc)
        .await
        .unwrap();

    tracing::info!("connected to anvil");

    todo!()
    // Ok((anvil, rpc))
}
