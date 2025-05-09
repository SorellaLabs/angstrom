use std::{collections::HashMap, pin::Pin, sync::Arc};

use alloy::primitives::Address;
use alloy_rpc_types::BlockId;
use angstrom::components::StromHandles;
use angstrom_eth::{
    handle::Eth,
    manager::{EthDataCleanser, EthEvent}
};
use angstrom_network::{PoolManagerBuilder, StromNetworkHandle, pool_manager::PoolHandle};
use angstrom_rpc::{OrderApi, api::OrderApiServer};
use angstrom_types::{
    block_sync::{BlockSyncProducer, GlobalBlockSync},
    contract_payloads::angstrom::{AngstromPoolConfigStore, UniswapAngstromRegistry},
    pair_with_price::PairsWithPrice,
    primitive::UniswapPoolRegistry,
    sol_bindings::testnet::TestnetHub,
    submission::SubmissionHandler,
    testnet::InitialTestnetState
};
use consensus::{AngstromValidator, ConsensusManager, ManagerNetworkDeps};
use futures::{Future, Stream, StreamExt};
use jsonrpsee::server::ServerBuilder;
use matching_engine::{MatchingManager, manager::MatcherHandle};
use order_pool::{PoolConfig, order_storage::OrderStorage};
use reth_provider::{BlockNumReader, CanonStateSubscriptions};
use reth_tasks::TaskExecutor;
use tracing::{Instrument, span};
use uniswap_v4::configure_uniswap_manager;
use validation::{
    common::{TokenPriceGenerator, WETH_ADDRESS},
    order::state::pools::AngstromPoolsTracker,
    validator::ValidationClient
};

use crate::{
    agents::AgentConfig,
    contracts::anvil::WalletProviderRpc,
    providers::{
        AnvilProvider, AnvilStateProvider, AnvilSubmissionProvider, WalletProvider,
        utils::StromContractInstance
    },
    types::{
        GlobalTestingConfig, SendingStromHandles, WithWalletProvider, config::TestingNodeConfig
    },
    validation::TestOrderValidator
};

pub struct AngstromNodeInternals<P> {
    pub rpc_port:         u64,
    pub state_provider:   AnvilProvider<P>,
    pub order_storage:    Arc<OrderStorage>,
    pub pool_handle:      PoolHandle,
    pub tx_strom_handles: SendingStromHandles,
    pub testnet_hub:      StromContractInstance
}

impl<P: WithWalletProvider> AngstromNodeInternals<P> {
    pub async fn new<G: GlobalTestingConfig, F>(
        node_config: TestingNodeConfig<G>,
        state_provider: AnvilProvider<P>,
        strom_handles: StromHandles,
        strom_network_handle: StromNetworkHandle,
        initial_validators: Vec<AngstromValidator>,
        inital_angstrom_state: InitialTestnetState,
        agents: Vec<F>,
        block_sync: GlobalBlockSync,
        executor: TaskExecutor
    ) -> eyre::Result<(
        Self,
        ConsensusManager<WalletProviderRpc, MatcherHandle, GlobalBlockSync>,
        TestOrderValidator<AnvilStateProvider<WalletProvider>>
    )>
    where
        F: for<'a> Fn(
            &'a InitialTestnetState,
            AgentConfig
        ) -> Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + 'a>>,
        F: Clone
    {
        let pool = strom_handles.get_pool_handle();
        let tx_strom_handles = (&strom_handles).into();

        let validation_client = ValidationClient(strom_handles.validator_tx);
        let matching_handle = MatchingManager::spawn(executor.clone(), validation_client.clone());

        let order_api = OrderApi::new(pool.clone(), executor.clone(), validation_client.clone());

        let block_number = BlockNumReader::best_block_number(&state_provider.state_provider())?;

        tracing::debug!(node_id = node_config.node_id, block_number, "creating strom internals");

        let uniswap_registry: UniswapPoolRegistry = inital_angstrom_state.pool_keys.clone().into();

        let angstrom_tokens = uniswap_registry
            .pools()
            .values()
            .flat_map(|pool| [pool.currency0, pool.currency1])
            .fold(HashMap::<Address, usize>::new(), |mut acc, x| {
                *acc.entry(x).or_default() += 1;
                acc
            });

        tracing::debug!("starting to load pool config store");
        let pool_config_store = Arc::new(
            AngstromPoolConfigStore::load_from_chain(
                inital_angstrom_state.angstrom_addr,
                BlockId::number(block_number),
                &state_provider.rpc_provider()
            )
            .await
            .map_err(|e| eyre::eyre!("{e}"))?
        );
        tracing::debug!("pool config loaded");

        let node_set = initial_validators.iter().map(|v| v.peer_id).collect();

        if node_config.is_devnet() {
            state_provider.mine_block().await?;
        }
        let b = state_provider
            .state_provider()
            .subscribe_to_canonical_state()
            .recv()
            .await
            .expect("startup sequence failed");
        tracing::debug!("got next block");

        let sub = state_provider
            .state_provider()
            .subscribe_to_canonical_state();

        let eth_handle = EthDataCleanser::spawn(
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

        tracing::debug!("spawned data cleaner");

        let block_number = b.tip().number;

        block_sync.clear();
        block_sync.set_block(block_number);

        tracing::debug!(node_id = node_config.node_id, block_number, "creating strom internals");

        let network_stream = Box::pin(eth_handle.subscribe_network())
            as Pin<Box<dyn Stream<Item = EthEvent> + Send + Sync>>;

        let uniswap_pool_manager = configure_uniswap_manager(
            state_provider.rpc_provider().into(),
            eth_handle.subscribe_cannon_state_notifications().await,
            uniswap_registry.clone(),
            block_number,
            block_sync.clone(),
            inital_angstrom_state.pool_manager_addr,
            network_stream
        )
        .await;
        tracing::debug!("uniswap configured");

        let uniswap_pools = uniswap_pool_manager.pools();
        executor.spawn_critical(
            "uniswap",
            Box::pin(uniswap_pool_manager.instrument(span!(
                tracing::Level::ERROR,
                "pool manager",
                node_config.node_id
            )))
        );

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
            token_price_update_stream
        ));

        let pool_storage = AngstromPoolsTracker::new(
            inital_angstrom_state.angstrom_addr,
            pool_config_store.clone()
        );

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

        let pool_config = PoolConfig {
            ids: uniswap_registry.pools().keys().cloned().collect::<Vec<_>>(),
            ..Default::default()
        };
        let order_storage = Arc::new(OrderStorage::new(&pool_config));

        let pool_handle = PoolManagerBuilder::new(
            validator.client.clone(),
            Some(order_storage.clone()),
            strom_network_handle.clone(),
            eth_handle.subscribe_network(),
            strom_handles.pool_rx,
            block_sync.clone()
        )
        .with_config(pool_config)
        .build_with_channels(
            executor.clone(),
            strom_handles.orderpool_tx,
            strom_handles.orderpool_rx,
            strom_handles.pool_manager_tx,
            block_number
        );

        let rpc_port = node_config.strom_rpc_port();
        let server = ServerBuilder::default()
            .build(format!("0.0.0.0:{}", rpc_port))
            .await?;

        let addr = server.local_addr()?;

        executor.spawn_critical(
            "rpc",
            Box::pin(async move {
                let server_handle = server.start(order_api.into_rpc());
                tracing::info!("rpc server started on: {}", addr);
                let _ = server_handle.stopped().await;
            })
        );

        let testnet_hub =
            TestnetHub::new(inital_angstrom_state.angstrom_addr, state_provider.rpc_provider());

        let pool_registry =
            UniswapAngstromRegistry::new(uniswap_registry.clone(), pool_config_store.clone());

        tracing::debug!("created testnet hub and uniswap registry");

        let anvil = AnvilSubmissionProvider {
            provider:         state_provider.rpc_provider(),
            angstrom_address: inital_angstrom_state.angstrom_addr
        };

        let mev_boost_provider = SubmissionHandler {
            node_provider: Arc::new(state_provider.rpc_provider()),
            submitters:    vec![Box::new(anvil)]
        };

        tracing::debug!("created mev boost provider");

        let consensus = ConsensusManager::new(
            ManagerNetworkDeps::new(
                strom_network_handle.clone(),
                eth_handle.subscribe_cannon_state_notifications().await,
                strom_handles.consensus_rx_op
            ),
            node_config.angstrom_signer(),
            initial_validators,
            order_storage.clone(),
            block_number,
            block_number,
            pool_registry,
            uniswap_pools.clone(),
            mev_boost_provider,
            matching_handle,
            block_sync.clone()
        );

        // init agents
        let agent_config = AgentConfig {
            uniswap_pools,
            agent_id: node_config.node_id,
            rpc_address: addr,
            current_block: block_number,
            state_provider: state_provider.state_provider()
        };

        futures::stream::iter(agents.into_iter())
            .map(|agent| (agent)(&inital_angstrom_state, agent_config.clone()))
            .buffer_unordered(4)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;

        tracing::info!("created consensus manager");

        block_sync.finalize_modules();
        Ok((
            Self {
                rpc_port,
                state_provider,
                order_storage,
                pool_handle,
                tx_strom_handles,
                testnet_hub
            },
            consensus,
            validator
        ))
    }
}
