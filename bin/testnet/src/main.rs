use std::time::Duration;

use alloy_primitives::Address;
use alloy_provider::Provider;
use alloy_sol_types::SolValue;
use angstrom::cli::{initialize_strom_handles, StromHandles};
use angstrom_eth::handle::Eth;
use angstrom_network::pool_manager::PoolManagerBuilder;
use angstrom_rpc::{api::OrderApiServer, OrderApi};
use clap::Parser;
use futures::StreamExt;
use jsonrpsee::server::ServerBuilder;
use reth_provider::test_utils::NoopProvider;
use reth_tasks::TokioTaskExecutor;
use sol_bindings::{sol::ContractBundle, testnet::TestnetHub};
use testnet::{
    anvil_utils::{spawn_anvil, AnvilEthDataCleanser},
    contract_setup::deploy_contract_and_create_pool,
    ported_reth_testnet_network::{connect_all_peers, StromPeer},
    rpc_state_provider::RpcStateProviderFactory
};
use tracing::{span, Instrument, Level};
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

    let (_anvil_handle, rpc) =
        spawn_anvil(cli_args.testnet_block_time_secs, cli_args.fork_url).await?;

    tracing::info!("deploying contracts to anvil");
    let addresses = deploy_contract_and_create_pool(rpc.clone()).await?;

    let rpc_wrapper = RpcStateProviderFactory::new(rpc.clone())?;
    let mut network_with_handles = vec![];
    let angstrom_addr = addresses.contract;

    for id in 0..=cli_args.nodes_in_network {
        let span = span!(Level::TRACE, "testnet node", id = id);
        let handles = initialize_strom_handles();
        let peer = StromPeer::new_fully_configed(
            NoopProvider::default(),
            Some(handles.pool_tx.clone()),
            Some(handles.consensus_tx_op.clone())
        )
        .instrument(span)
        .await;
        let pk = peer.get_node_public_key();
        network_with_handles.push((pk, peer, handles));
    }
    connect_all_peers(&mut network_with_handles).await;

    for id in 0..cli_args.nodes_in_network {
        let (_, peer, handles) = network_with_handles.pop().expect("unreachable");
        spawn_testnet_node(rpc_wrapper.clone(), peer, handles, None, angstrom_addr, id).await?;
    }

    let (_, peer, handles) = network_with_handles.pop().expect("unreachable");
    // spawn the node with rpc
    tokio::spawn(async move {
        spawn_testnet_node(
            rpc_wrapper.clone(),
            peer,
            handles,
            Some(cli_args.port),
            angstrom_addr,
            cli_args.nodes_in_network
        )
        .await
        .unwrap();
    });

    let testnet = TestnetHub::new(addresses.contract, rpc.clone());

    loop {
        tokio::time::sleep(Duration::from_secs(11)).await;
        let orders = ContractBundle::generate_random_bundles(10);
        let hashes = orders.get_filled_hashes();
        tracing::info!("submitting a angstrom bundle with hashes: {:#?}", hashes);
        let tx_hash = testnet
            .execute(orders.abi_encode().into())
            .send()
            .await?
            .watch()
            .await?;

        tracing::info!(?tx_hash, "tx hash with angstrom contract sent");
    }
}

pub async fn spawn_testnet_node(
    rpc_wrapper: RpcStateProviderFactory,
    network: StromPeer<NoopProvider>,
    handles: StromHandles,
    port: Option<u16>,
    contract_address: Address,
    id: u64
) -> eyre::Result<()> {
    let span = span!(Level::ERROR, "testnet node", id = id);
    let pool = handles.get_pool_handle();
    let executor: TokioTaskExecutor = Default::default();

    let rpc_w = rpc_wrapper.clone();
    let balls = rpc_wrapper
        .provider
        .clone()
        .subscribe_blocks()
        .await?
        .into_stream()
        .map(move |block| {
            let cloned_block = block.clone();
            let rpc = rpc_w.clone();
            async move {
                let number = cloned_block.header.number.unwrap();
                let mut res = vec![];
                for hash in cloned_block.transactions.hashes() {
                    let Ok(Some(tx)) = rpc
                        .provider
                        .get_transaction_by_hash(*hash)
                        .await else { continue };
                    res.push(tx);
                }
                (number, res)
            }
        })
        .buffer_unordered(10);

    let order_api = OrderApi { pool: pool.clone() };
    let eth_handle = AnvilEthDataCleanser::spawn(
        executor.clone(),
        contract_address,
        handles.eth_tx,
        handles.eth_rx,
        balls,
        7,
        span
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
