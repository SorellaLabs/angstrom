use std::{collections::HashSet, pin::Pin, sync::Arc, time::Duration};

use alloy::{
    network::{Ethereum, EthereumWallet},
    node_bindings::{Anvil, AnvilInstance},
    primitives::Address,
    providers::Provider
};
use alloy_primitives::aliases::I24;
use alloy_rpc_types::{BlockId, BlockNumberOrTag, Filter};
use alloy_sol_types::SolEvent;
use angstrom::components::initialize_strom_handles;
use angstrom_amm_quoter::{QuoterHandle, QuoterManager};
use angstrom_eth::{
    handle::Eth,
    manager::{EthDataCleanser, EthEvent}
};
use angstrom_network::{PoolManagerBuilder, pool_manager::PoolHandle};
use angstrom_rpc::{
    ConsensusApi, OrderApi,
    api::{ConsensusApiServer, OrderApiServer}
};
use angstrom_types::{
    block_sync::{BlockSyncProducer, GlobalBlockSync},
    contract_bindings::{
        angstrom::Angstrom::PoolKey,
        controller_v_1::ControllerV1::{self, PoolConfigured, PoolRemoved}
    },
    contract_payloads::angstrom::{AngstromPoolConfigStore, UniswapAngstromRegistry},
    pair_with_price::PairsWithPrice,
    primitive::{AngstromSigner, UniswapPoolRegistry, try_init_with_chain_id, *},
    submission::{ChainSubmitterHolder, SubmissionHandler}
};
use consensus::{AngstromValidator, ConsensusHandler, ConsensusManager, ManagerNetworkDeps};
use futures::{Stream, StreamExt};
use jsonrpsee::server::ServerBuilder;
use matching_engine::MatchingManager;
use order_pool::{OrderPoolHandle, PoolConfig};
use reth_provider::CanonStateSubscriptions;
use reth_tasks::TaskExecutor;
use telemetry::blocklog::BlockLog;
use telemetry_recorder::TelemetryMessage;
use tracing::{Instrument, span};
use uniswap_v4::configure_uniswap_manager;
use validation::{
    common::TokenPriceGenerator, init_validation_replay, validator::ValidationClient
};

use super::fake_network::FakeNetwork;
use crate::providers::{
    AnvilProvider, AnvilStateProvider, AnvilSubmissionProvider, WalletProvider
};

/// Replay Runner
///
/// The Replay Runner allows us to take any snapshot of a block and replay it
/// in order to allow us to debug quickly.
pub struct ReplayRunner {
    #[allow(dead_code)]
    anvil:          AnvilInstance,
    anvil_provider: AnvilProvider<WalletProvider>,
    fake_network:   FakeNetwork,
    block_log:      BlockLog,
    pool_handle:    PoolHandle
}

impl ReplayRunner {
    pub async fn run(self) -> eyre::Result<()> {
        // At this point all the setup should be done.
        // what we do now is send the message of a new block to the eth_manager
        // and then start sending the rest of the orders.
        let update = self
            .block_log
            .eth_snapshot
            .as_ref()
            .unwrap()
            .chain_update
            .clone();

        match update {
            angstrom_eth::telemetry::AngstromChainUpdate::New(chain) => self
                .anvil_provider
                .state_provider()
                .canon_state_tx
                .send(reth_provider::CanonStateNotification::Commit { new: chain })
                .unwrap(),
            angstrom_eth::telemetry::AngstromChainUpdate::Reorg { new, old } => self
                .anvil_provider
                .state_provider()
                .canon_state_tx
                .send(reth_provider::CanonStateNotification::Reorg { old, new })
                .unwrap()
        };

        let mut last_timestamp = self.block_log.eth_snapshot.as_ref().unwrap().timestamp;

        for next_event in self.block_log.events() {
            match next_event {
                TelemetryMessage::NewOrder { origin, order, timestamp, .. } => {
                    tracing::info!("NewOrder event playing back");
                    let delta = timestamp.signed_duration_since(last_timestamp);
                    let sleep_duration =
                        Duration::from_micros(delta.abs().num_microseconds().unwrap() as u64);
                    tokio::time::sleep(sleep_duration).await;
                    last_timestamp = *timestamp;
                    tracing::info!("Slept for time delta, applying new order");

                    self.pool_handle
                        .new_order(*origin, order.clone())
                        .await
                        .unwrap();
                }
                TelemetryMessage::CancelOrder { cancel, timestamp, .. } => {
                    tracing::info!("CancelOrder event playing back");
                    let delta = timestamp.signed_duration_since(last_timestamp);
                    let sleep_duration =
                        Duration::from_micros(delta.abs().num_microseconds().unwrap() as u64);
                    tokio::time::sleep(sleep_duration).await;
                    last_timestamp = *timestamp;
                    tracing::info!("CancelOrder sleep completed");
                    self.pool_handle.cancel_order(cancel.clone()).await;
                }
                TelemetryMessage::Consensus { event, timestamp, .. } => {
                    tracing::info!("Consensus event playing back");
                    let delta = timestamp.signed_duration_since(last_timestamp);
                    let sleep_duration =
                        Duration::from_micros(delta.abs().num_microseconds().unwrap() as u64);
                    tokio::time::sleep(sleep_duration).await;
                    tracing::info!("Consensus event sleep completed");
                    last_timestamp = *timestamp;

                    self.fake_network
                        .to_consensus_manager
                        .as_ref()
                        .unwrap()
                        .send(event.clone())
                        .unwrap();
                }
                _ => ()
            }
        }
        tracing::info!("everything completed");

        Ok(())
    }

    pub async fn new(
        block_log: BlockLog,
        fork_url: String,
        rpc_port: u16,
        executor: TaskExecutor
    ) -> eyre::Result<Self> {
        let chain_id = block_log.constants.as_ref().unwrap().chain_id;
        try_init_with_chain_id(chain_id).unwrap();

        let http_port = 8545u16; // Use HTTP instead of IPC for now

        // Clone necessary values before moving into async block
        let block_num = block_log.blocknum();
        let http_port_clone = http_port;

        // First try with forking, then fallback to local if fork fails
        tracing::info!("Attempting to spawn anvil with fork URL: {}", fork_url);

        let anvil = Anvil::new()
            .arg("--host")
            .arg("0.0.0.0")
            .fork_block_number(block_num)
            .chain_id(chain_id)
            .fork(fork_url)
            .port(http_port_clone)
            .arg("--ipc")
            .arg("--no-mining")
            .arg("--timeout")
            .arg("60000")
            .spawn();

        // Wait for anvil to be ready socket to be created.
        tokio::time::sleep(Duration::from_secs(5)).await;

        let node_addr = block_log.constants.as_ref().unwrap().node_address();
        let angstrom_signer = AngstromSigner::for_address(node_addr);
        let sk = angstrom_signer.clone().into_signer();

        let wallet = EthereumWallet::new(sk.clone());
        let rpc = alloy::providers::builder::<Ethereum>()
            .with_recommended_fillers()
            .wallet(wallet.clone())
            .connect("/tmp/anvil.ipc")
            .await
            .unwrap();

        tracing::info!("connected to anvil");

        let wallet_provider = WalletProvider::new_with_provider(rpc.clone(), sk);
        let state_provider = AnvilStateProvider::new(wallet_provider);
        let anvil_provider = AnvilProvider::new(state_provider, None, None);

        let angstrom_address = *ANGSTROM_ADDRESS.get().unwrap();
        let controller = *CONTROLLER_V1_ADDRESS.get().unwrap();
        let deploy_block = *ANGSTROM_DEPLOYED_BLOCK.get().unwrap();
        let gas_token = *GAS_TOKEN_ADDRESS.get().unwrap();
        let pool_manager = *POOL_MANAGER_ADDRESS.get().unwrap();

        let mut strom_handles = initialize_strom_handles();

        // for rpc
        let pool = strom_handles.get_pool_handle();

        let periphery_c = ControllerV1::new(*CONTROLLER_V1_ADDRESS.get().unwrap(), rpc.clone());
        let node_set = periphery_c
            .nodes()
            .call()
            .await
            .unwrap()
            .into_iter()
            .collect::<HashSet<_>>();

        let validation_client = ValidationClient(strom_handles.validator_tx);
        let matching_handle = MatchingManager::spawn(executor.clone(), validation_client.clone());
        let consensus_client = ConsensusHandler(strom_handles.consensus_tx_rpc.clone());

        let consensus_api = ConsensusApi::new(consensus_client.clone(), executor.clone());

        let amm_quoter = QuoterHandle(strom_handles.quoter_tx.clone());
        let order_api =
            OrderApi::new(pool.clone(), executor.clone(), validation_client.clone(), amm_quoter);

        // We set -1 as the start of the replay will be triggering new block transition.
        let block_number = block_num - 1;

        let global_block_sync = GlobalBlockSync::new(block_number);

        let pool_config_store = Arc::new(
            AngstromPoolConfigStore::load_from_chain(
                angstrom_address,
                BlockId::Number(BlockNumberOrTag::Latest),
                &rpc
            )
            .await
            .unwrap()
        );
        let pools = fetch_angstrom_pools(
            deploy_block as usize,
            block_number as usize,
            angstrom_address,
            controller,
            &rpc
        )
        .await;

        let uniswap_registry: UniswapPoolRegistry = pools.into();

        let sub = anvil_provider
            .state_provider()
            .subscribe_to_canonical_state();

        let eth_snap = block_log.eth_snapshot.as_ref().unwrap();

        let eth_handle = EthDataCleanser::spawn(
            angstrom_address,
            controller,
            sub,
            executor.clone(),
            strom_handles.eth_tx,
            strom_handles.eth_rx,
            eth_snap.angstrom_tokens.clone(),
            eth_snap.pool_store.clone(),
            global_block_sync.clone(),
            eth_snap.node_set.clone(),
            vec![]
        )
        .unwrap();

        tracing::debug!("spawned data cleaner");

        let network_stream = Box::pin(eth_handle.subscribe_network())
            as Pin<Box<dyn Stream<Item = EthEvent> + Send + Sync>>;

        let uniswap_pool_manager = configure_uniswap_manager::<_, 50>(
            rpc.clone().into(),
            eth_handle.subscribe_cannon_state_notifications().await,
            uniswap_registry.clone(),
            block_number + 1,
            global_block_sync.clone(),
            pool_manager,
            network_stream
        )
        .await;
        tracing::debug!("uniswap configured");

        let uniswap_pools = uniswap_pool_manager.pools();
        executor.spawn_critical(
            "uniswap",
            Box::pin(
                uniswap_pool_manager.instrument(span!(tracing::Level::ERROR, "pool manager",))
            )
        );

        let token_conversion =
            if let Some((prev_prices, base_wei)) = block_log.gas_price_snapshot.as_ref() {
                println!("Using snapshot");
                TokenPriceGenerator::from_snapshot(
                    uniswap_pools.clone(),
                    prev_prices.clone(),
                    gas_token,
                    *base_wei
                )
            } else {
                TokenPriceGenerator::new(
                    Arc::new(anvil_provider.rpc_provider()),
                    block_number,
                    uniswap_pools.clone(),
                    gas_token,
                    Some(1)
                )
                .await
                .expect("failed to start price generator")
            };

        let token_price_update_stream = anvil_provider.state_provider().canonical_state_stream();
        let token_price_update_stream = Box::pin(PairsWithPrice::into_price_update_stream(
            angstrom_address,
            token_price_update_stream,
            Arc::new(anvil_provider.rpc_provider())
        ));

        let user_account = block_log
            .validation_snapshot
            .as_ref()
            .unwrap()
            .state
            .clone();

        init_validation_replay(
            anvil_provider.state_provider(),
            block_number,
            angstrom_address,
            node_addr,
            token_price_update_stream,
            uniswap_pools.clone(),
            token_conversion,
            pool_config_store.clone(),
            strom_handles.validator_rx,
            |validator| validator.set_user_account(user_account)
        );

        let pool_config = PoolConfig {
            ids: uniswap_registry.pools().keys().cloned().collect::<Vec<_>>(),
            ..Default::default()
        };

        let pool_snapshot = block_log.order_pool_snapshot.as_ref().unwrap().clone();
        let order_storage = Arc::new(pool_snapshot.order_storage.clone());

        let fake_network = FakeNetwork::new(
            Some(strom_handles.pool_tx),
            Some(strom_handles.consensus_tx_op),
            strom_handles.eth_handle_rx.take().unwrap()
        );

        let network_handle = fake_network.handle.clone();

        let pool_handle = PoolManagerBuilder::new(
            validation_client.clone(),
            Some(order_storage.clone()),
            network_handle.clone(),
            eth_handle.subscribe_network(),
            strom_handles.pool_rx,
            global_block_sync.clone()
        )
        .with_config(pool_config)
        .build_with_channels(
            executor.clone(),
            strom_handles.orderpool_tx,
            strom_handles.orderpool_rx,
            strom_handles.pool_manager_tx,
            block_number,
            |order_indexer| {
                // Order storage is set above.
                order_indexer.set_tracker(pool_snapshot.order_tracker);
            }
        );

        let server = ServerBuilder::default()
            .build(format!("0.0.0.0:{}", rpc_port))
            .await?;

        let addr = server.local_addr()?;

        executor.spawn_critical(
            "rpc",
            Box::pin(async move {
                let mut rpcs = order_api.into_rpc();
                rpcs.merge(consensus_api.into_rpc()).unwrap();
                let server_handle = server.start(rpcs);
                tracing::info!("rpc server started on: {}", addr);
                let _ = server_handle.stopped().await;
            })
        );

        let pool_registry =
            UniswapAngstromRegistry::new(uniswap_registry.clone(), pool_config_store.clone());

        let anvil_sub =
            AnvilSubmissionProvider { provider: anvil_provider.rpc_provider(), angstrom_address };

        let mev_boost_provider = SubmissionHandler {
            node_provider: Arc::new(anvil_provider.rpc_provider()),
            submitters:    vec![Box::new(ChainSubmitterHolder::new(
                anvil_sub,
                angstrom_signer.clone()
            ))]
        };

        tracing::debug!("created mev boost provider");
        let validators = node_set
            .into_iter()
            // use same weight for all validators
            .map(|addr| AngstromValidator::new(addr, 100))
            .collect::<Vec<_>>();

        let consensus = ConsensusManager::new(
            ManagerNetworkDeps::new(
                network_handle.clone(),
                eth_handle.subscribe_cannon_state_notifications().await,
                strom_handles.consensus_rx_op
            ),
            angstrom_signer,
            validators,
            order_storage.clone(),
            block_number,
            block_number,
            pool_registry,
            uniswap_pools.clone(),
            mev_boost_provider,
            matching_handle,
            global_block_sync.clone(),
            strom_handles.consensus_rx_rpc,
            None
        );
        executor.spawn_critical_with_graceful_shutdown_signal("consensus", move |grace| {
            consensus.run_till_shutdown(grace)
        });

        // spin up amm quoter
        let amm = QuoterManager::new(
            global_block_sync.clone(),
            order_storage.clone(),
            strom_handles.quoter_rx,
            uniswap_pools.clone(),
            rayon::ThreadPoolBuilder::default()
                .num_threads(2)
                .build()
                .expect("failed to build rayon thread pool"),
            Duration::from_millis(100)
        );

        executor.spawn_critical("amm quoting service", amm);

        tracing::info!("created consensus manager");
        global_block_sync.finalize_modules();

        Ok(Self { anvil, anvil_provider, fake_network, block_log, pool_handle })
    }
}

async fn fetch_angstrom_pools<P>(
    // the block angstrom was deployed at
    mut deploy_block: usize,
    end_block: usize,
    angstrom_address: Address,
    controller_address: Address,
    db: &P
) -> Vec<PoolKey>
where
    P: Provider
{
    let mut filters = vec![];

    loop {
        let this_end_block = std::cmp::min(deploy_block + 99_999, end_block);

        if this_end_block == deploy_block {
            break;
        }

        let filter = Filter::new()
            .from_block(deploy_block as u64)
            .to_block(this_end_block as u64)
            .address(controller_address);

        filters.push(filter);

        deploy_block = std::cmp::min(end_block, this_end_block);
    }

    let logs = futures::stream::iter(filters)
        .map(|filter| async move {
            db.get_logs(&filter)
                .await
                .unwrap()
                .into_iter()
                .collect::<Vec<_>>()
        })
        .buffered(10)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    logs.into_iter()
        .fold(HashSet::new(), |mut set, log| {
            if let Ok(pool) = PoolConfigured::decode_log(&log.clone().into_inner()) {
                let pool_key = PoolKey {
                    currency0:   pool.asset0,
                    currency1:   pool.asset1,
                    fee:         pool.bundleFee,
                    tickSpacing: I24::try_from_be_slice(&{
                        let bytes = pool.tickSpacing.to_be_bytes();
                        let mut a = [0u8; 3];
                        a[1..3].copy_from_slice(&bytes);
                        a
                    })
                    .unwrap(),
                    hooks:       angstrom_address
                };

                set.insert(pool_key);
                return set;
            }

            if let Ok(pool) = PoolRemoved::decode_log(&log.clone().into_inner()) {
                let pool_key = PoolKey {
                    currency0:   pool.asset0,
                    currency1:   pool.asset1,
                    fee:         pool.feeInE6,
                    tickSpacing: pool.tickSpacing,
                    hooks:       angstrom_address
                };

                set.remove(&pool_key);
                return set;
            }
            set
        })
        .into_iter()
        .collect::<Vec<_>>()
}
