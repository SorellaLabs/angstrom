use alloy::{
    node_bindings::Anvil,
    transports::http::{Client, Http}
};
use alloy_provider::{builder, RootProvider};

pub async fn spawn_anvil(
    block_time: u64,
    fork_url: String
) -> eyre::Result<RootProvider<Http<Client>>> {
    let anvil = Anvil::new()
        .block_time(block_time)
        .fork_block_number(20214717)
        .fork(fork_url)
        .chain_id(1)
        .try_spawn()?;
    let endpoint = anvil.endpoint_url();
    tracing::info!(?endpoint);
    let rpc = builder().on_http(endpoint);

    tracing::info!("connected to anvil");

    Ok(rpc)
}
