use alloy::node_bindings::Anvil;
use alloy_provider::{builder, RootProvider};
use alloy_pubsub::PubSubFrontend;
use alloy_transport_ws::WsConnect;

pub async fn spawn_anvil(block_time: u64) -> eyre::Result<RootProvider<PubSubFrontend>> {
    let anvil = Anvil::new().block_time(block_time).try_spawn()?;
    let endpoint = anvil.ws_endpoint_url();
    let connect = WsConnect::new(endpoint);
    let rpc = builder().on_ws(connect).await?;

    Ok(rpc)
}
