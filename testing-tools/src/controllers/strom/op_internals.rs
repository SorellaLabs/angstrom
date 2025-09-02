use std::{pin::Pin, sync::Arc, time::Duration};

use alloy_rpc_types::BlockId;
use angstrom_amm_quoter::{QuoterHandle, RollupQuoterManager};
use angstrom_cli::{handles::RollupHandles, manager::RollupManager};
use angstrom_eth::{
    handle::Eth,
    manager::{EthDataCleanser, EthEvent}
};
use angstrom_rpc::{OrderApi, api::OrderApiServer};
use angstrom_types::{
    block_sync::GlobalBlockSync,
    contract_payloads::angstrom::{AngstromPoolConfigStore, UniswapAngstromRegistry},
    pair_with_price::PairsWithPrice,
    primitive::UniswapPoolRegistry,
    submission::SubmissionHandler,
    testnet::InitialTestnetState
};
use futures::{Future, Stream, StreamExt};
use jsonrpsee::server::ServerBuilder;
use matching_engine::MatchingManager;
use op_alloy_network::Optimism;
use order_pool::{PoolConfig, order_storage::OrderStorage};
use pool_manager::rollup::RollupPoolManager;
use reth_optimism_primitives::OpPrimitives;
use reth_provider::{BlockNumReader, CanonStateSubscriptions};
use reth_tasks::TaskExecutor;
use tokio_stream::wrappers::BroadcastStream;
use tracing::{Instrument, span};
use uniswap_v4::{DEFAULT_TICKS, configure_uniswap_manager};
use validation::{
    common::{TokenPriceGenerator, WETH_ADDRESS},
    order::state::pools::AngstromPoolsTracker,
    validator::ValidationClient
};

use crate::{
    agents::AgentConfig,
    providers::AnvilProvider,
    types::{GlobalTestingConfig, WithWalletProvider, config::TestingNodeConfig},
    validation::TestOrderValidator
};

/// Internal builder for the minimal single-node OP testnet pipeline.
/// Spawns data cleanser, uniswap manager, pricing, validator, pool manager,
/// RPC server, AMM quoter, agents, and the rollup driver.
pub struct OpNodeInternals<P> {
    pub state_provider: AnvilProvider<P, Optimism, OpPrimitives>
}

impl<P: WithWalletProvider<Optimism>> OpNodeInternals<P> {
    pub async fn new<G, F>(
        node_config: TestingNodeConfig<G>,
        state_provider: AnvilProvider<P, Optimism, OpPrimitives>,
        strom_handles: RollupHandles,
        inital_angstrom_state: InitialTestnetState,
        agents: Vec<F>,
        executor: TaskExecutor,
        shutdown_rx: tokio::sync::watch::Receiver<bool>
    ) -> eyre::Result<Self>
    where
        G: GlobalTestingConfig,
        F: for<'a> Fn(
                &'a InitialTestnetState,
                AgentConfig<Optimism, OpPrimitives>
            ) -> Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + 'a>>
            + Clone
    {
        // Matching engine + validation client
        let validation_client = ValidationClient(strom_handles.validator_tx.clone());
        let matching_handle = MatchingManager::spawn(executor.clone(), validation_client.clone());

        // Load registry + pool config store at current canonical block
        let block_number = BlockNumReader::best_block_number(&state_provider.state_provider())?;
        let uniswap_registry: UniswapPoolRegistry = inital_angstrom_state.pool_keys.clone().into();
        let pool_config_store = Arc::new(
            AngstromPoolConfigStore::load_from_chain(
                inital_angstrom_state.angstrom_addr,
                BlockId::number(block_number),
                &state_provider.rpc_provider()
            )
            .await
            .map_err(|e| eyre::eyre!("{e}"))?
        );

        // Wait for first canonical state update
        let _ = state_provider
            .state_provider()
            .subscribe_to_canonical_state()
            .recv()
            .await;

        let sub = state_provider
            .state_provider()
            .subscribe_to_canonical_state();

        // Spawn chain data cleanser
        let angstrom_tokens = uniswap_registry
            .pools()
            .values()
            .flat_map(|pool| [pool.currency0, pool.currency1])
            .fold(
                std::collections::HashMap::<alloy::primitives::Address, usize>::new(),
                |mut acc, x| {
                    *acc.entry(x).or_default() += 1;
                    acc
                }
            );

        let node_set = std::iter::once(node_config.address()).collect();
        let block_sync = GlobalBlockSync::new(block_number);
        let eth_handle = EthDataCleanser::<_, OpPrimitives>::spawn(
            inital_angstrom_state.angstrom_addr,
            inital_angstrom_state.controller_addr,
            sub,
            executor.clone(),
            strom_handles.eth_tx,
            strom_handles.eth_rx,
            angstrom_tokens,
            pool_config_store.clone(),
            block_sync.clone(),
            node_set,
            vec![]
        )
        .unwrap();

        // Uniswap pool manager
        let network_stream = Box::pin(eth_handle.subscribe_network())
            as Pin<Box<dyn Stream<Item = EthEvent> + Send + Sync>>;
        let uniswap_pool_manager = configure_uniswap_manager::<_, _, DEFAULT_TICKS>(
            state_provider.rpc_provider().into(),
            eth_handle.subscribe_cannon_state_notifications().await,
            uniswap_registry.clone(),
            block_number,
            block_sync.clone(),
            inital_angstrom_state.pool_manager_addr,
            network_stream
        )
        .await;
        let uniswap_pools = uniswap_pool_manager.pools();
        {
            let mut shutdown_rx_uniswap = shutdown_rx.clone();
            let fut = uniswap_pool_manager.instrument(span!(
                tracing::Level::ERROR,
                "pool manager",
                node_config.node_id
            ));
            executor.spawn_critical_with_graceful_shutdown_signal(
                "uniswap",
                move |grace| async move {
                    tokio::pin!(fut);
                    tokio::select! {
                        _ = &mut fut => {}
                        _ = grace => {}
                        _ = shutdown_rx_uniswap.changed() => {}
                    }
                }
            );
        }

        // Token conversion and price updates
        let token_conversion = TokenPriceGenerator::new(
            Arc::new(state_provider.rpc_provider()),
            block_number,
            uniswap_pools.clone(),
            WETH_ADDRESS,
            Some(1)
        )
        .await
        .expect("failed to start price generator");
        let token_price_update_stream = state_provider.state_provider().canonical_state_stream();
        let token_price_update_stream = Box::pin(PairsWithPrice::into_price_update_stream(
            inital_angstrom_state.angstrom_addr,
            token_price_update_stream,
            Arc::new(state_provider.rpc_provider())
        ));

        let pool_storage = AngstromPoolsTracker::new(
            inital_angstrom_state.angstrom_addr,
            pool_config_store.clone()
        );

        // Validation
        let validator = TestOrderValidator::new(
            state_provider.state_provider(),
            validation_client.clone(),
            strom_handles.validator_rx,
            inital_angstrom_state.angstrom_addr,
            node_config.address(),
            uniswap_pools.clone(),
            token_conversion,
            token_price_update_stream,
            pool_storage.clone(),
            node_config.node_id
        )
        .await?;
        {
            let mut shutdown_rx_validator = shutdown_rx.clone();
            let validator_task = validator;
            executor.spawn_critical_with_graceful_shutdown_signal(
                "validator",
                move |grace| async move {
                    tokio::pin!(validator_task);
                    tokio::select! {
                        _ = &mut validator_task => {}
                        _ = grace => {}
                        _ = shutdown_rx_validator.changed() => {}
                    }
                }
            );
        }

        // Pool manager and storage
        let pool_config = PoolConfig {
            ids: uniswap_registry.pools().keys().cloned().collect::<Vec<_>>(),
            ..Default::default()
        };
        let order_storage = Arc::new(OrderStorage::new(&pool_config));
        let pool_handle = RollupPoolManager::new(
            validation_client.clone(),
            Some(order_storage.clone()),
            eth_handle.subscribe_network(),
            block_sync.clone(),
            Duration::from_secs(12)
        )
        .with_config(pool_config)
        .build_with_channels(
            executor.clone(),
            strom_handles.orderpool_tx.clone(),
            strom_handles.orderpool_rx,
            strom_handles.pool_manager_tx.clone(),
            block_number,
            |_| {}
        );

        // RPC server binds to port 0 and announces chosen port
        let server = ServerBuilder::default().build("127.0.0.1:0").await?;
        let addr = server.local_addr()?;
        let amm_quoter = QuoterHandle(strom_handles.quoter_tx.clone());
        let order_api = OrderApi::new(
            pool_handle.clone(),
            executor.clone(),
            validation_client.clone(),
            amm_quoter
        );
        {
            let mut shutdown_rx_rpc = shutdown_rx.clone();
            executor.spawn_critical_with_graceful_shutdown_signal("rpc", move |grace| async move {
                let rpcs = order_api.into_rpc();
                let server_handle = server.start(rpcs);
                tracing::info!("rpc server started on: {}", addr);
                tokio::select! {
                    _ = server_handle.clone().stopped() => {}
                    _ = grace => { let _ = server_handle.stop(); }
                    _ = shutdown_rx_rpc.changed() => { let _ = server_handle.stop(); }
                }
            });
        }

        // spin up amm quoter
        {
            let mut shutdown_rx_amm = shutdown_rx.clone();
            let quoter = RollupQuoterManager::new(
                block_sync.clone(),
                order_storage.clone(),
                strom_handles.quoter_rx,
                uniswap_pools.clone(),
                rayon::ThreadPoolBuilder::default()
                    .num_threads(2)
                    .build()
                    .expect("failed to build rayon thread pool"),
                Duration::from_millis(100)
            );
            executor.spawn_critical_with_graceful_shutdown_signal(
                "amm quoting service",
                move |grace| async move {
                    tokio::pin!(quoter);
                    tokio::select! {
                        _ = &mut quoter => {}
                        _ = grace => {}
                        _ = shutdown_rx_amm.changed() => {}
                    }
                }
            );
        }

        // init agents
        let agent_config: AgentConfig<Optimism, OpPrimitives> = AgentConfig {
            uniswap_pools:  uniswap_pools.clone(),
            agent_id:       node_config.node_id,
            rpc_address:    addr,
            current_block:  block_number,
            state_provider: state_provider.state_provider()
        };
        futures::stream::iter(agents.into_iter())
            .map(|agent| (agent)(&inital_angstrom_state, agent_config.clone()))
            .buffer_unordered(4)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;

        // Use RollupManager to build/submit bundles
        let signer = node_config.angstrom_signer();
        let pool_registry =
            UniswapAngstromRegistry::new(uniswap_registry.clone(), pool_config_store);
        let submission_handler = SubmissionHandler::new(
            state_provider.rpc_provider().into(),
            &[],
            inital_angstrom_state.angstrom_addr,
            signer.clone()
        );
        let block_time = Duration::from_secs(12);
        let canon_stream =
            BroadcastStream::new(eth_handle.subscribe_cannon_state_notifications().await);
        let driver = RollupManager::new(
            block_number,
            block_time,
            canon_stream,
            block_sync.clone(),
            order_storage.clone(),
            pool_registry,
            uniswap_pools.clone(),
            Arc::new(submission_handler),
            matching_handle.clone(),
            signer
        );
        executor.spawn_critical_with_graceful_shutdown_signal("rollup driver", move |grace| {
            driver.run_till_shutdown(grace)
        });

        Ok(Self { state_provider })
    }
}
