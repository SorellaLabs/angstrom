use std::time::Duration;

use anvil::{eth::EthApi, spawn, NodeConfig, NodeHandle};

pub async fn spawn_anvil_on_url(
    url: String,
    block_time: Duration
) -> eyre::Result<(EthApi, NodeHandle)> {
    let config = NodeConfig::default()
        .with_blocktime(Some(block_time))
        .with_eth_rpc_url(Some(url));

    Ok(spawn(config).await)
}
