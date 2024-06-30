use std::time::Duration;

use angstrom::cli::{initialize_strom_handles, StromHandles};
use angstrom_eth::handle::{Eth, EthHandle};
use angstrom_network::{network::StromNetworkHandle, pool_manager::PoolManagerBuilder};
use angstrom_rpc::{api::OrderApiServer, OrderApi};
use clap::Parser;
use jsonrpsee::server::ServerBuilder;
use reth_metrics::common::mpsc::UnboundedMeteredSender;
use reth_tasks::TokioTaskExecutor;
use testnet::utils::{ported_reth_testnet_network::StromPeer, RpcStateProviderFactory};
use tokio::sync::mpsc::unbounded_channel;
use validation::init_validation;

#[derive(Parser)]
#[clap(about = "Angstrom Testnet Node")]
struct Cli {
    /// port for the rpc for submitting transactions.
    #[clap(short, long, default_value_t = 4200)]
    port:                    u16,
    /// url that anvil is set to. only really useful for overriding on url
    /// conflict
    #[clap(short, long, default_value = "http://localhost:8545")]
    anvil_rpc_url:           String,
    /// the speed in which anvil will mine blocks.
    #[clap(short, long, default_value = "12")]
    testnet_block_time_secs: u64,
    /// the amount of testnet nodes that will be spawned and connected to.
    /// NOTE: only 1 rpc will be connected currently for submissions.
    /// this will change in the future but is good enough for testing currently
    #[clap(short, long, default_value = "3")]
    nodes_in_network:        u64
}

const CACHE_VALIDATION_SIZE: usize = 100_000_000;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let env_filter = tracing_subscriber::EnvFilter::from_default_env();
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;
    let cli_args = Cli::parse();

    let (_eth_api, anvil_handle) = testnet::utils::anvil_manager::spawn_anvil_on_url(
        cli_args.anvil_rpc_url,
        Duration::from_secs(cli_args.testnet_block_time_secs)
    )
    .await?;

    let ipc_handle = anvil_handle
        .ipc_provider()
        .expect("couldn't connect to ipc handle for anvil");

    let rpc_wrapper = RpcStateProviderFactory::new(ipc_handle)?;

    for _ in 0..cli_args.nodes_in_network {
        spawn_testnet_node(rpc_wrapper.clone(), None).await?;
    }

    // spawn the node with rpc
    spawn_testnet_node(rpc_wrapper.clone(), Some(cli_args.port)).await?;

    Ok(())
}

pub async fn spawn_testnet_node(
    rpc_wrapper: RpcStateProviderFactory,
    network: StromPeer<RpcStateProviderFactory>,
    handles: StromHandles,
    port: Option<u16>
) -> eyre::Result<()> {
    let pool = handles.get_pool_handle();

    let order_api = OrderApi { pool: pool.clone() };
    let eth_handle = EthHandle::new(handles.eth_tx);

    let validator =
        init_validation(rpc_wrapper, CACHE_VALIDATION_SIZE, eth_handle.subscribe_network_stream());

    let (network_tx, _network_rx) = unbounded_channel();
    let network_handle = StromNetworkHandle::new(
        Default::default(),
        UnboundedMeteredSender::new(network_tx, "mock network")
    );

    let executor: TokioTaskExecutor = Default::default();

    let _pool_handle = PoolManagerBuilder::new(
        validator.clone(),
        network_handle.clone(),
        eth_handle.subscribe_network(),
        handles.pool_rx
    )
    .build_with_channels(executor, handles.orderpool_tx, handles.orderpool_rx);

    if let Some(port) = port {
        let server = ServerBuilder::default()
            .build(format!("127.0.0.1:{}", port))
            .await?;

        let addr = server.local_addr().unwrap();
        println!("rpc server started on: {}", addr);

        let server_handle = server.start(order_api.into_rpc());
        let _ = server_handle.stopped().await;
    }

    Ok(())
}
