use std::sync::Arc;

use alloy::{providers::Provider, pubsub::PubSubFrontend};
use alloy_rpc_types::Transaction;
use angstrom::components::StromHandles;
use angstrom_eth::handle::Eth;
use angstrom_network::{pool_manager::PoolHandle, PoolManagerBuilder, StromNetworkHandle};
use angstrom_rpc::{api::OrderApiServer, OrderApi};
use angstrom_types::{
    block_sync::GlobalBlockSync,
    contract_payloads::angstrom::{AngstromPoolConfigStore, UniswapAngstromRegistry},
    mev_boost::{MevBoostProvider, SubmitTx},
    pair_with_price::PairsWithPrice,
    primitive::UniswapPoolRegistry,
    sol_bindings::testnet::TestnetHub,
    testnet::InitialTestnetState
};
use consensus::{AngstromValidator, ConsensusManager, ManagerNetworkDeps};
use futures::{StreamExt, TryStreamExt};
use jsonrpsee::server::ServerBuilder;
use matching_engine::{configure_uniswap_manager, manager::MatcherHandle, MatchingManager};
use order_pool::{order_storage::OrderStorage, PoolConfig};
use reth_provider::CanonStateSubscriptions;
use reth_tasks::TokioTaskExecutor;
use tokio_stream::wrappers::BroadcastStream;
use validation::{
    common::TokenPriceGenerator, order::state::pools::AngstromPoolsTracker,
    validator::ValidationClient
};

use crate::{
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
    pub testnet_hub:      StromContractInstance,
    pub validator:        TestOrderValidator<AnvilStateProvider<WalletProvider>>
}

impl<P: WithWalletProvider> AngstromDevnetNodeInternals<P> {
    pub async fn new<G: GlobalTestingConfig>(
        node_config: TestingNodeConfig<G>,
        state_provider: AnvilProvider<P>,
        strom_handles: StromHandles,
        strom_network_handle: StromNetworkHandle,
        initial_validators: Vec<AngstromValidator>,
        block_rx: BroadcastStream<(u64, Vec<Transaction>)>,
        inital_angstrom_state: InitialTestnetState
    ) -> eyre::Result<(
        Self,
        ConsensusManager<WalletProviderRpc, PubSubFrontend, MatcherHandle, GlobalBlockSync>
    )> {
        let pool = strom_handles.get_pool_handle();
        let executor: TokioTaskExecutor = Default::default();
        let tx_strom_handles = (&strom_handles).into();

        let validation_client = ValidationClient(strom_handles.validator_tx);
        let matching_handle = MatchingManager::spawn(executor.clone(), validation_client.clone());

        let order_api = OrderApi::new(pool.clone(), executor.clone(), validation_client);

        let eth_handle = AnvilEthDataCleanser::spawn(
            node_config.node_id,
            executor.clone(),
            inital_angstrom_state.angstrom_addr,
            strom_handles.eth_tx,
            strom_handles.eth_rx,
            block_rx.into_stream().map(|v| v.unwrap()),
            7
        )
        .await?;

        let block_number = state_provider
            .rpc_provider()
            .get_block_number()
            .await
            .unwrap();
        tracing::debug!(block_number, "creating strom internals");
        let block_sync = GlobalBlockSync::new(block_number);

        let uniswap_registry: UniswapPoolRegistry = inital_angstrom_state.pool_keys.into();

        let pool_config_store = Arc::new(
            AngstromPoolConfigStore::load_from_chain(
                inital_angstrom_state.angstrom_addr,
                block_number.into(),
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
            inital_angstrom_state.pool_manager_addr,
            false
        )
        .await;

        let uniswap_pools = uniswap_pool_manager.pools();
        tokio::spawn(async move { uniswap_pool_manager.watch_state_changes().await });

        let token_conversion = TokenPriceGenerator::new(
            Arc::new(state_provider.rpc_provider()),
            block_number,
            uniswap_pools.clone(),
            Some(0)
        )
        .await
        .expect("failed to start price generator");

        let token_price_update_stream = state_provider.state_provider().canonical_state_stream();
        let token_price_update_stream = Box::pin(PairsWithPrice::into_price_update_stream(
            inital_angstrom_state.angstrom_addr,
            token_price_update_stream
        ));

        let validator = TestOrderValidator::new(
            state_provider.state_provider(),
            inital_angstrom_state.angstrom_addr,
            inital_angstrom_state.pool_manager_addr,
            node_config.address(),
            uniswap_pools.clone(),
            token_conversion,
            token_price_update_stream,
            pool_config_store.clone()
        )
        .await;

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
            AngstromPoolsTracker::new(
                inital_angstrom_state.angstrom_addr,
                pool_config_store.clone()
            ),
            strom_handles.pool_manager_tx
        );

        let rpc_port = node_config.strom_rpc_port();
        let server = ServerBuilder::default()
            .build(format!("127.0.0.1:{}", rpc_port))
            .await?;

        let addr = server.local_addr().unwrap();

        tokio::spawn(async move {
            let server_handle = server.start(order_api.into_rpc());
            tracing::info!("rpc server started on: {}", addr);
            let _ = server_handle.stopped().await;
        });

        let testnet_hub =
            TestnetHub::new(inital_angstrom_state.angstrom_addr, state_provider.rpc_provider());

        let pool_registry =
            UniswapAngstromRegistry::new(uniswap_registry.clone(), pool_config_store.clone());

        let override_sender = vec![Arc::new(Box::new(AnvilSubmissionProvider {
            provider: state_provider.rpc_provider()
        }) as Box<dyn SubmitTx>)];

        let mev_boost_provider = MevBoostProvider::new_from_raw(
            Arc::new(state_provider.rpc_provider()),
            override_sender
        );

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
            uniswap_pools,
            mev_boost_provider,
            matching_handle,
            block_sync.clone()
        );

        Ok((
            Self {
                rpc_port,
                state_provider,
                order_storage,
                pool_handle,
                tx_strom_handles,
                testnet_hub,
                validator
            },
            consensus
        ))
    }
}
