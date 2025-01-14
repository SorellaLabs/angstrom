use std::{pin::Pin, sync::Arc};

use alloy_rpc_types::{BlockId, Transaction};
use angstrom::components::StromHandles;
use angstrom_eth::handle::Eth;
use angstrom_network::{pool_manager::PoolHandle, PoolManagerBuilder, StromNetworkHandle};
use angstrom_rpc::{api::OrderApiServer, OrderApi};
use angstrom_types::{
    block_sync::{BlockSyncProducer, GlobalBlockSync},
    contract_payloads::angstrom::{AngstromPoolConfigStore, UniswapAngstromRegistry},
    mev_boost::{MevBoostProvider, SubmitTx},
    pair_with_price::PairsWithPrice,
    primitive::UniswapPoolRegistry,
    sol_bindings::testnet::TestnetHub,
    testnet::InitialTestnetState
};
use consensus::{AngstromValidator, ConsensusManager, ManagerNetworkDeps};
use futures::{Future, Stream, StreamExt, TryStreamExt};
use jsonrpsee::server::ServerBuilder;
use matching_engine::{configure_uniswap_manager, manager::MatcherHandle, MatchingManager};
use order_pool::{order_storage::OrderStorage, PoolConfig};
use reth_provider::{BlockNumReader, CanonStateSubscriptions};
use reth_tasks::TokioTaskExecutor;
use tokio_stream::wrappers::BroadcastStream;
use tracing::{span, Instrument};
use validation::{
    common::TokenPriceGenerator, order::state::pools::AngstromPoolsTracker,
    validator::ValidationClient
};

use crate::{
    agents::AgentConfig,
    contracts::anvil::WalletProviderRpc,
    providers::{
        utils::StromContractInstance, AnvilEthDataCleanser, AnvilProvider, AnvilStateProvider,
        AnvilSubmissionProvider, WalletProvider
    },
    types::{
        config::TestingNodeConfig, GlobalTestingConfig, SendingStromHandles, WithWalletProvider
    },
    validation::TestOrderValidator
};

pub struct AngstromDevnetNodeInternals<P> {
    pub rpc_port:         u64,
    pub state_provider:   AnvilProvider<P>,
    pub order_storage:    Arc<OrderStorage>,
    pub pool_handle:      PoolHandle,
    pub tx_strom_handles: SendingStromHandles,
    pub testnet_hub:      StromContractInstance
}

impl<P: WithWalletProvider> AngstromDevnetNodeInternals<P> {
    pub async fn new<G: GlobalTestingConfig, F>(
        node_config: TestingNodeConfig<G>,
        state_provider: AnvilProvider<P>,
        strom_handles: StromHandles,
        strom_network_handle: StromNetworkHandle,
        initial_validators: Vec<AngstromValidator>,
        block_rx: BroadcastStream<(u64, Vec<Transaction>)>,
        inital_angstrom_state: InitialTestnetState,
        agents: Vec<F>,
        block_sync: GlobalBlockSync
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
        let executor: TokioTaskExecutor = Default::default();
        let tx_strom_handles = (&strom_handles).into();

        let validation_client = ValidationClient(strom_handles.validator_tx);
        let matching_handle = MatchingManager::spawn(executor.clone(), validation_client.clone());

        let order_api = OrderApi::new(pool.clone(), executor.clone(), validation_client.clone());

        let block_subscription: Pin<
            Box<dyn Stream<Item = (u64, Vec<Transaction>)> + Unpin + Send>
        > = if node_config.is_devnet() {
            Box::pin(block_rx.into_stream().map(|v| v.unwrap()))
        } else {
            Box::pin(state_provider.subscribe_blocks().await?)
        };

        let block_number = BlockNumReader::best_block_number(&state_provider.state_provider())?;
        block_sync.set_block(block_number);

        let eth_handle = AnvilEthDataCleanser::spawn(
            node_config.node_id,
            executor.clone(),
            inital_angstrom_state.angstrom_addr,
            strom_handles.eth_tx,
            strom_handles.eth_rx,
            block_subscription,
            7,
            block_sync.clone()
        )
        .await?;
        // wait for new block then clear all proposals and init rest.
        // this gives us 12 seconds so we can ensure all nodes are on the same update

        let b = state_provider
            .state_provider()
            .subscribe_to_canonical_state()
            .recv()
            .await
            .expect("startup sequence failed");

        block_sync.clear();
        let block_number = b.tip().number;

        tracing::debug!(block_number, "creating strom internals");

        let uniswap_registry: UniswapPoolRegistry = inital_angstrom_state.pool_keys.clone().into();

        let pool_config_store = Arc::new(
            AngstromPoolConfigStore::load_from_chain(
                inital_angstrom_state.angstrom_addr,
                BlockId::latest(),
                &state_provider.rpc_provider()
            )
            .await
            .map_err(|e| eyre::eyre!("{e}"))?
        );

        let uniswap_pool_manager = configure_uniswap_manager(
            state_provider.rpc_provider().into(),
            state_provider
                .state_provider()
                .subscribe_to_canonical_state(),
            uniswap_registry.clone(),
            block_number,
            block_sync.clone(),
            inital_angstrom_state.pool_manager_addr
        )
        .await;

        let uniswap_pools = uniswap_pool_manager.pools();
        tokio::spawn(uniswap_pool_manager.instrument(span!(
            tracing::Level::ERROR,
            "pool manager",
            node_config.node_id
        )));

        let token_conversion = TokenPriceGenerator::new(
            Arc::new(state_provider.rpc_provider()),
            block_number,
            uniswap_pools.clone(),
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
            pool_storage,
            strom_handles.pool_manager_tx
        );

        let rpc_port = node_config.strom_rpc_port();
        let server = ServerBuilder::default()
            .build(format!("0.0.0.0:{}", rpc_port))
            .await?;

        let addr = server.local_addr()?;

        tokio::spawn(async move {
            let server_handle = server.start(order_api.into_rpc());
            tracing::info!("rpc server started on: {}", addr);
            let _ = server_handle.stopped().await;
        });

        let testnet_hub =
            TestnetHub::new(inital_angstrom_state.angstrom_addr, state_provider.rpc_provider());

        let pool_registry =
            UniswapAngstromRegistry::new(uniswap_registry.clone(), pool_config_store.clone());

        tracing::debug!("created testnet hub and uniswap registry");

        let anvil = AnvilSubmissionProvider { provider: state_provider.rpc_provider() };

        let mev_boost_provider = MevBoostProvider::new_from_raw(
            Arc::new(state_provider.rpc_provider()),
            vec![Arc::new(Box::new(anvil) as Box<dyn SubmitTx>)]
        );

        tracing::debug!("created mev boost provider");

        let consensus = ConsensusManager::new(
            ManagerNetworkDeps::new(
                strom_network_handle.clone(),
                state_provider
                    .state_provider()
                    .subscribe_to_canonical_state(),
                strom_handles.consensus_rx_op
            ),
            node_config.angstrom_signer(),
            initial_validators,
            order_storage.clone(),
            block_number,
            inital_angstrom_state.angstrom_addr,
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
