use alloy_primitives::Address;
use alloy_provider::Provider;
use angstrom::cli::{initialize_strom_handles, StromHandles};
use angstrom_eth::handle::Eth;
use angstrom_network::pool_manager::PoolManagerBuilder;
use angstrom_rpc::{api::OrderApiServer, OrderApi};
use clap::Parser;
use jsonrpsee::server::ServerBuilder;
use reth_provider::test_utils::NoopProvider;
use reth_tasks::TokioTaskExecutor;
use testnet::utils::{
    ported_reth_testnet_network::{connect_all_peers, StromPeer},
    AnvilEthDataCleanser, RpcStateProviderFactory
};
use validation::init_validation;

#[derive(Parser)]
#[clap(about = "
Angstrom Anvil Testnet.
Anvil must be installed on the system in order to spin up the \
                testnode. 
To install run `curl -L https://foundry.paradigm.xyz | bash`. then run foundryup to install anvil
    ")]
struct Cli {
    /// port for the rpc for submitting transactions.
    #[clap(short, long, default_value_t = 4200)]
    port:                    u16,
    /// the speed in which anvil will mine blocks.
    #[clap(short, long, default_value = "12")]
    testnet_block_time_secs: u64,
    /// the amount of testnet nodes that will be spawned and connected to.
    /// NOTE: only 1 rpc will be connected currently for submissions.
    /// this will change in the future but is good enough for testing currently
    #[clap(short, long, default_value = "3")]
    nodes_in_network:        u64,
    /// used to tell anvil where to fork from. default is the reth node on the
    /// reth1 server.
    #[clap(short, long, default_value = "localhost:8489")]
    fork_url:                String
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

    let (anvil_handle, rpc) = testnet::utils::anvil_manager::spawn_anvil(
        cli_args.testnet_block_time_secs,
        cli_args.fork_url
    )
    .await?;

    let rpc_wrapper = RpcStateProviderFactory::new(rpc)?;
    let mut network_with_handles = vec![];
    let addr = Address::ZERO;

    for _ in 0..=cli_args.nodes_in_network {
        let handles = initialize_strom_handles();
        let peer = StromPeer::new_fully_configed(
            NoopProvider::default(),
            Some(handles.pool_tx.clone()),
            Some(handles.consensus_tx_op.clone())
        )
        .await;
        let pk = peer.get_node_public_key();
        network_with_handles.push((pk, peer, handles));
    }
    connect_all_peers(&mut network_with_handles).await;

    for _ in 0..cli_args.nodes_in_network {
        let (_, peer, handles) = network_with_handles.pop().expect("unreachable");
        spawn_testnet_node(rpc_wrapper.clone(), peer, handles, None, addr).await?;
    }

    let (_, peer, handles) = network_with_handles.pop().expect("unreachable");
    // spawn the node with rpc
    spawn_testnet_node(rpc_wrapper.clone(), peer, handles, Some(cli_args.port), addr).await?;

    Ok(())
}

pub async fn spawn_testnet_node(
    rpc_wrapper: RpcStateProviderFactory,
    network: StromPeer<NoopProvider>,
    handles: StromHandles,
    port: Option<u16>,
    contract_address: Address
) -> eyre::Result<()> {
    let pool = handles.get_pool_handle();
    let executor: TokioTaskExecutor = Default::default();

    let order_api = OrderApi { pool: pool.clone() };
    let eth_handle = AnvilEthDataCleanser::spawn(
        executor.clone(),
        contract_address,
        handles.eth_tx,
        handles.eth_rx,
        rpc_wrapper
            .provider
            .clone()
            .subscribe_blocks()
            .await?
            .into_stream(),
        7
    )
    .await?;

    let validator =
        init_validation(rpc_wrapper, CACHE_VALIDATION_SIZE, eth_handle.subscribe_network_stream());

    let network_handle = network.handle.clone();

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
