//! Example of using proc macro to generate working client and server.

use alloy_primitives::Bytes;
use alloy_rlp::encode as rlp_encode;
use angstrom::cli::initialize_strom_handles;
use angstrom_eth::handle::{Eth, EthHandle};
use angstrom_network::{network::StromNetworkHandle, pool_manager::PoolManagerBuilder};
use angstrom_rpc::{api::OrderApiServer, OrderApi};
use clap::Parser;
use jsonrpsee::server::ServerBuilder;
use reth_metrics::common::mpsc::UnboundedMeteredSender;
use reth_tasks::TokioTaskExecutor;
use testing_tools::{
    mocks::eth_events::MockEthEventHandle, order_pool::TestnetOrderPool,
    type_generator::orders::generate_rand_valid_limit_order
};
use tokio::sync::mpsc::unbounded_channel;
use validation::init_validation;

use crate::rpc_state_provider::RpcStateProviderFactory;

mod order_pool;
mod rpc_state_provider;

#[derive(Parser)]
#[clap(about = "Angstrom Testnet Node")]
struct Cli {
    #[clap(short, long, default_value_t = 4200)]
    port:          u16,
    #[clap(short, long, default_value = "http://localhost:8545")]
    local_rpc_url: String
}

const CACHE_VALIDATION_SIZE: usize = 100_000_000;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let env_filter = tracing_subscriber::EnvFilter::from_default_env();
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let cli_args = Cli::parse();
    let rpc_wrapper = RpcStateProviderFactory::new(&cli_args.local_rpc_url)?;
    let handles = initialize_strom_handles();
    let pool = handles.get_pool_handle();

    let order_api = OrderApi { pool: pool.clone() };
    let eth_handle = EthHandle::new(handles.eth_tx);

    let validator =
        init_validation(rpc_wrapper, CACHE_VALIDATION_SIZE, eth_handle.subscribe_network_stream());

    let (network_tx, network_rx) = unbounded_channel();
    let network_handle = StromNetworkHandle::new(
        Default::default(),
        UnboundedMeteredSender::new(network_tx, "mock network")
    );

    let executor: TokioTaskExecutor = Default::default();

    let pool_handle = PoolManagerBuilder::new(
        validator.clone(),
        network_handle.clone(),
        eth_handle.subscribe_network(),
        handles.pool_rx
    )
    .build_with_channels(executor, handles.orderpool_tx, handles.orderpool_rx);

    let server = ServerBuilder::default()
        .build(format!("127.0.0.1:{}", cli_args.port))
        .await?;

    let addr = server.local_addr().unwrap();
    println!("rpc server started on: {}", addr);

    let server_handle = server.start(order_api.into_rpc());
    let _ = server_handle.stopped().await;

    Ok(())
}
