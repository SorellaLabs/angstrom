use std::{pin::Pin, sync::Arc, time::Duration};

use alloy::{providers::Provider, signers::local::PrivateKeySigner};
use alloy_rpc_types::BlockId;
use angstrom_amm_quoter::{QuoterHandle, RollupQuoterManager};
use angstrom_eth::{
    handle::Eth,
    manager::{EthDataCleanser, EthEvent}
};
use angstrom_rpc::{OrderApi, api::OrderApiServer};
use angstrom_types::{
    block_sync::GlobalBlockSync,
    contract_payloads::angstrom::{AngstromPoolConfigStore, UniswapAngstromRegistry},
    pair_with_price::PairsWithPrice,
    primitive::{AngstromSigner, UniswapPoolRegistry},
    submission::SubmissionHandler,
    testnet::InitialTestnetState
};
use futures::{Future, Stream, StreamExt};
use jsonrpsee::server::ServerBuilder;
use matching_engine::{MatchingEngineHandle, MatchingManager};
use order_pool::{PoolConfig, order_storage::OrderStorage};
use pool_manager::rollup::RollupPoolManager;
use reth_provider::{BlockNumReader, CanonStateSubscriptions};
use reth_tasks::TaskExecutor;
use tracing::{Instrument, span};
use uniswap_v4::{DEFAULT_TICKS, configure_uniswap_manager};
use validation::{
    common::{TokenPriceGenerator, WETH_ADDRESS},
    order::state::pools::AngstromPoolsTracker,
    validator::ValidationClient
};

use crate::{
    agents::AgentConfig,
    providers::{AnvilProvider, WalletProvider},
    types::{GlobalTestingConfig, WithWalletProvider, config::TestingNodeConfig}
};

/// Minimal OP testnet node: no custom networking or consensus.
pub struct OpTestnetNode<P, G> {
    state_provider: AnvilProvider<P>,
    _init_state:    InitialTestnetState,
    config:         TestingNodeConfig<G>,
    /// Internal shutdown signal used to gracefully stop background tasks
    shutdown_tx:    tokio::sync::watch::Sender<bool>
}

impl<P, G> OpTestnetNode<P, G>
where
    P: WithWalletProvider,
    G: GlobalTestingConfig
{
    pub async fn new<F>(
        node_config: TestingNodeConfig<G>,
        state_provider: AnvilProvider<P>,
        inital_angstrom_state: InitialTestnetState,
        agents: Vec<F>,
        executor: TaskExecutor
    ) -> eyre::Result<Self>
    where
        F: for<'a> Fn(
                &'a InitialTestnetState,
                AgentConfig
            ) -> Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + 'a>>
            + Clone
    {
        // Bootstrap minimal single-node pipeline mirroring Angstrom testnet
        let _start_block = state_provider
            .rpc_provider()
            .get_block_number()
            .await
            .unwrap();

        // Minimal channel setup (no consensus or networking)
        let (eth_tx, eth_rx) = tokio::sync::mpsc::channel(100);
        let (pool_manager_tx, _) = tokio::sync::broadcast::channel(100);
        let (orderpool_tx, orderpool_rx) = tokio::sync::mpsc::unbounded_channel();
        let (validator_tx, validator_rx) = tokio::sync::mpsc::unbounded_channel();
        let (quoter_tx, quoter_rx) = tokio::sync::mpsc::channel(1000);
        // Shutdown signal for graceful task termination
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        let validation_client = ValidationClient(validator_tx.clone());
        let matching_handle = MatchingManager::spawn(executor.clone(), validation_client.clone());

        // Load pool config and registries
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

        // Ensure we have at least one canonical state update
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
        let eth_handle = EthDataCleanser::spawn(
            inital_angstrom_state.angstrom_addr,
            inital_angstrom_state.controller_addr,
            sub,
            executor.clone(),
            eth_tx,
            eth_rx,
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

        let validator = crate::validation::TestOrderValidator::new(
            state_provider.state_provider(),
            validation_client.clone(),
            validator_rx,
            inital_angstrom_state.angstrom_addr,
            node_config.address(),
            uniswap_pools.clone(),
            token_conversion,
            token_price_update_stream,
            pool_storage.clone(),
            node_config.node_id
        )
        .await?;

        // Spawn validation task so it consumes requests until graceful shutdown
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
            orderpool_tx.clone(),
            orderpool_rx,
            pool_manager_tx.clone(),
            block_number,
            |_| {}
        );

        // RPC server
        let rpc_port = node_config.strom_rpc_port();
        let server = ServerBuilder::default()
            .build(format!("127.0.0.1:{rpc_port}"))
            .await?;
        let addr = server.local_addr()?;

        let amm_quoter = QuoterHandle(quoter_tx.clone());
        let order_api = OrderApi::new(
            pool_handle.clone(),
            executor.clone(),
            validation_client.clone(),
            amm_quoter
        );
        let mut shutdown_rx_rpc = shutdown_rx.clone();
        executor.spawn_critical_with_graceful_shutdown_signal("rpc", move |grace| async move {
            let rpcs = order_api.into_rpc();
            let server_handle = server.start(rpcs);
            tracing::info!("rpc server started on: {}", addr);
            tokio::select! {
                _ = server_handle.clone().stopped() => {}
                _ = grace => {
                    let _ = server_handle.stop();
                }
                _ = shutdown_rx_rpc.changed() => {
                    let _ = server_handle.stop();
                }
            }
        });

        // AMM quoting service (rollup)
        let amm = RollupQuoterManager::new(
            block_sync.clone(),
            order_storage.clone(),
            quoter_rx,
            uniswap_pools.clone(),
            rayon::ThreadPoolBuilder::default()
                .num_threads(2)
                .build()
                .expect("failed to build rayon thread pool"),
            Duration::from_millis(100)
        );
        {
            let mut shutdown_rx_amm = shutdown_rx.clone();
            executor.spawn_critical_with_graceful_shutdown_signal(
                "amm quoting service",
                move |grace| async move {
                    tokio::pin!(amm);
                    tokio::select! {
                        _ = &mut amm => {}
                        _ = grace => {}
                        _ = shutdown_rx_amm.changed() => {}
                    }
                }
            );
        }

        // Agents
        let uniswap_pools_for_agents = uniswap_pools.clone();
        let agent_config = AgentConfig {
            uniswap_pools:  uniswap_pools_for_agents,
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

        // Minimal rollup driver: build and submit bundles on new blocks
        let provider_for_submit = state_provider.rpc_provider();
        let angstrom_addr = inital_angstrom_state.angstrom_addr;
        let signer = node_config.angstrom_signer();
        let pool_registry =
            UniswapAngstromRegistry::new(uniswap_registry.clone(), pool_config_store);
        let submission = SubmissionHandler::new(
            provider_for_submit.clone().into(),
            &[],
            angstrom_addr,
            signer.clone()
        );
        let uniswap_pools_clone = uniswap_pools.clone();
        let order_storage_clone = order_storage.clone();

        let mut shutdown_rx_driver = shutdown_rx.clone();
        executor.spawn_critical_with_graceful_shutdown_signal(
            "op-rollup-driver",
            move |grace| async move {
                let mut canon = eth_handle.subscribe_cannon_state_notifications().await;
                loop {
                    tokio::select! {
                        res = canon.recv() => {
                            if res.is_err() { break }
                        }
                        _ = &mut grace.clone() => break,
                        _ = shutdown_rx_driver.changed() => break,
                    }
                    // Build pool snapshots
                    let pool_snapshots = uniswap_pools_clone
                        .iter()
                        .filter_map(|item| {
                            let key = item.key();
                            let pool = item.value();
                            let (token_a, token_b, snapshot) = pool.read().ok()?.fetch_pool_snapshot().ok()?;
                            let entry = pool_registry.get_ang_entry(key)?;
                            Some((*key, (token_a, token_b, snapshot, entry.store_index as u16)))
                        })
                        .collect::<std::collections::HashMap<_, _>>();

                    // Fetch current orders
                    let all_orders = order_storage_clone.get_all_orders_with_ingoing_cancellations();
                    let limit = all_orders.limit.clone();
                    let searcher = all_orders.searcher.clone();

                    // Solve pools and build bundle
                    if let Ok((solutions, details)) = matching_handle
                        .solve_pools(limit.clone(), searcher.clone(), pool_snapshots.clone())
                        .await
                    {
                        if let Ok(bundle) = angstrom_types::contract_payloads::angstrom::AngstromBundle::from_pool_solutions(
                            solutions,
                            order_storage_clone.get_all_orders_with_ingoing_cancellations(),
                            &pool_snapshots,
                            details
                        ) {
                            let current_block = provider_for_submit.get_block_number().await.unwrap_or(0);
                            let _ = submission.submit_tx(signer.clone(), Some(bundle), current_block + 1).await;
                        }
                    }
                }
            }
        );

        Ok(Self { state_provider, _init_state: inital_angstrom_state, config: node_config, shutdown_tx })
    }

    pub fn state_provider(&self) -> &AnvilProvider<P> {
        &self.state_provider
    }

    pub fn get_sk(&self) -> AngstromSigner<PrivateKeySigner> {
        self.config.angstrom_signer()
    }

    pub async fn testnet_future(self) {
        // Keep the node alive (no networking/consensus to drive here)
        futures::future::pending::<()>().await;
    }

    /// Signal all internal tasks to shut down gracefully.
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }

    /// Cloneable sender to trigger shutdown from external code.
    pub fn shutdown_sender(&self) -> tokio::sync::watch::Sender<bool> {
        self.shutdown_tx.clone()
    }
}

// Convenience alias for OP Angstrom use-site
pub type OpWalletNode<G> = OpTestnetNode<WalletProvider, G>;
