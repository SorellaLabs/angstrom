use std::sync::{atomic::AtomicBool, Arc};

use alloy::{
    eips::{BlockId, BlockNumberOrTag},
    network::Network,
    primitives::{aliases::U24, Signed},
    providers::Provider,
    pubsub::PubSubFrontend,
    transports::Transport
};
use alloy_primitives::{Address, BlockNumber};
use angstrom::cli::StromHandles;
use angstrom_eth::handle::Eth;
use angstrom_network::{pool_manager::PoolHandle, PoolManagerBuilder, StromNetworkHandle};
use angstrom_rpc::{api::OrderApiServer, OrderApi};
use angstrom_types::{
    contract_bindings::angstrom::Angstrom::PoolKey,
    contract_payloads::angstrom::{AngstromPoolConfigStore, UniswapAngstromRegistry},
    pair_with_price::PairsWithPrice,
    primitive::{PoolId as AngstromPoolId, UniswapPoolRegistry},
    sol_bindings::testnet::TestnetHub
};
use consensus::{AngstromValidator, ConsensusManager, ManagerNetworkDeps, Signer};
use futures::StreamExt;
use jsonrpsee::server::ServerBuilder;
use matching_engine::cfmm::uniswap::{
    pool::EnhancedUniswapPool, pool_data_loader::DataLoader, pool_manager::UniswapPoolManager,
    pool_providers::canonical_state_adapter::CanonicalStateAdapter
};
use order_pool::{order_storage::OrderStorage, PoolConfig};
use reth_provider::{CanonStateNotifications, CanonStateSubscriptions};
use reth_tasks::TokioTaskExecutor;
use secp256k1::SecretKey;
use validation::{
    order::state::{pools::AngstromPoolsTracker, token_pricing::TokenPriceGenerator},
    validator::ValidationClient
};

use crate::{
    anvil_state_provider::{
        state_provider_factory::{RpcStateProviderFactory, RpcStateProviderFactoryWrapper},
        utils::StromContractInstance,
        AnvilEthDataCleanser
    },
    contracts::deploy_contract_and_create_pool,
    network::TestnetConsensusFuture,
    testnet_controllers::AngstromTestnetConfig,
    types::SendingStromHandles,
    validation::TestOrderValidator
};

pub struct AngstromTestnetNodeInternals {
    pub rpc_port:         u64,
    pub state_provider:   RpcStateProviderFactoryWrapper,
    pub order_storage:    Arc<OrderStorage>,
    pub pool_handle:      PoolHandle,
    pub tx_strom_handles: SendingStromHandles,
    pub testnet_hub:      StromContractInstance,
    pub validator:        TestOrderValidator<RpcStateProviderFactory>,
    _consensus:           TestnetConsensusFuture<PubSubFrontend>,
    _consensus_running:   Arc<AtomicBool>
}

impl AngstromTestnetNodeInternals {
    pub async fn new(
        testnet_node_id: u64,
        strom_handles: StromHandles,
        strom_network_handle: StromNetworkHandle,
        secret_key: SecretKey,
        config: AngstromTestnetConfig,
        initial_validators: Vec<AngstromValidator>
    ) -> eyre::Result<Self> {
        tracing::debug!("connecting to state provider");
        let state_provider =
            RpcStateProviderFactoryWrapper::spawn_new(config, testnet_node_id).await?;
        tracing::info!("connected to state provider");

        tracing::debug!("deploying contracts to anvil");
        let addresses =
            deploy_contract_and_create_pool(state_provider.provider().provider()).await?;
        tracing::info!("deployed contracts to anvil");

        let angstrom_addr = addresses.contract;
        let pools = vec![PoolKey {
            currency0:   addresses.token0,
            currency1:   addresses.token1,
            fee:         U24::from(0),
            tickSpacing: Signed::<24, 1>::from_limbs([5]),
            hooks:       addresses.hooks
        }];

        let pool = strom_handles.get_pool_handle();
        let executor: TokioTaskExecutor = Default::default();
        let tx_strom_handles = (&strom_handles).into();

        let rpc_w = state_provider.provider();
        let state_stream = state_provider
            .provider()
            .provider()
            .subscribe_blocks()
            .await?
            .into_stream()
            .map(move |block| {
                let cloned_block = block.clone();
                let rpc = rpc_w.clone();
                async move {
                    let number = cloned_block.header.number;
                    let mut res = vec![];
                    for hash in cloned_block.transactions.hashes() {
                        let Ok(Some(tx)) = rpc.provider().get_transaction_by_hash(hash).await
                        else {
                            continue
                        };
                        res.push(tx);
                    }
                    (number, res)
                }
            })
            .buffer_unordered(10);

        let order_api = OrderApi::new(
            pool.clone(),
            executor.clone(),
            ValidationClient(strom_handles.validator_tx)
        );

        let eth_handle = AnvilEthDataCleanser::spawn(
            testnet_node_id,
            executor.clone(),
            angstrom_addr,
            strom_handles.eth_tx,
            strom_handles.eth_rx,
            state_stream,
            7
        )
        .await?;

        let block_id = state_provider
            .provider()
            .provider()
            .get_block_number()
            .await
            .unwrap();

        let uniswap_registry: UniswapPoolRegistry = pools.into();

        let uniswap_pool_manager = configure_uniswap_manager(
            state_provider.provider().provider().into(),
            state_provider.provider().subscribe_to_canonical_state(),
            uniswap_registry.clone(),
            block_id
        )
        .await;

        let uniswap_pools = uniswap_pool_manager.pools();
        tokio::spawn(async move { uniswap_pool_manager.watch_state_changes().await });

        let token_conversion = TokenPriceGenerator::new(
            state_provider.provider().provider().into(),
            block_id,
            uniswap_pools.clone()
        )
        .await
        .expect("failed to start price generator");

        let token_price_update_stream = state_provider.provider().canonical_state_stream();
        let token_price_update_stream =
            PairsWithPrice::into_price_update_stream(Address::default(), token_price_update_stream)
                .boxed();

        let pool_config_store = Arc::new(
            AngstromPoolConfigStore::load_from_chain(
                angstrom_addr,
                BlockId::Number(BlockNumberOrTag::Number(block_id)),
                &state_provider.provider().provider()
            )
            .await
            .unwrap()
        );

        let validator = TestOrderValidator::new(
            state_provider.provider(),
            angstrom_addr,
            uniswap_pools.clone(),
            token_conversion,
            token_price_update_stream,
            pool_config_store.clone()
        )
        .await;

        let pool_config = PoolConfig::default();
        let order_storage = Arc::new(OrderStorage::new(&pool_config));

        let pool_handle = PoolManagerBuilder::new(
            validator.client.clone(),
            Some(order_storage.clone()),
            strom_network_handle.clone(),
            eth_handle.subscribe_network(),
            strom_handles.pool_rx
        )
        .with_config(pool_config)
        .build_with_channels(
            executor.clone(),
            strom_handles.orderpool_tx,
            strom_handles.orderpool_rx,
            AngstromPoolsTracker::new(angstrom_addr, pool_config_store.clone()),
            strom_handles.pool_manager_tx
        );

        let rpc_port = config.rpc_port_with_node_id(testnet_node_id);
        let server = ServerBuilder::default()
            .build(format!("127.0.0.1:{}", rpc_port))
            .await?;

        let addr = server.local_addr().unwrap();

        tokio::spawn(async move {
            let server_handle = server.start(order_api.into_rpc());
            tracing::info!("rpc server started on: {}", addr);
            let _ = server_handle.stopped().await;
        });

        let testnet_hub = TestnetHub::new(angstrom_addr, state_provider.provider().provider());

        let pool_registry = UniswapAngstromRegistry::new(uniswap_registry, pool_config_store);

        let consensus_handle = ConsensusManager::new(
            ManagerNetworkDeps::new(
                strom_network_handle.clone(),
                state_provider.provider().subscribe_to_canonical_state(),
                strom_handles.consensus_rx_op
            ),
            Signer::new(secret_key),
            initial_validators,
            order_storage.clone(),
            state_provider
                .provider()
                .provider()
                .get_block_number()
                .await?,
            pool_registry,
            uniswap_pools.clone(),
            state_provider.provider().provider()
        );

        let _consensus_running = Arc::new(AtomicBool::new(true));

        let _consensus = TestnetConsensusFuture::new(
            testnet_node_id,
            consensus_handle,
            _consensus_running.clone()
        );

        Ok(Self {
            rpc_port,
            state_provider,
            order_storage,
            pool_handle,
            tx_strom_handles,
            testnet_hub,
            validator,
            _consensus,
            _consensus_running
        })
    }
}

async fn configure_uniswap_manager<T: Transport + Clone, N: Network>(
    provider: Arc<impl Provider<T, N>>,
    state_notification: CanonStateNotifications,
    uniswap_pool_registry: UniswapPoolRegistry,
    current_block: BlockNumber
) -> UniswapPoolManager<CanonicalStateAdapter, DataLoader<AngstromPoolId>, AngstromPoolId> {
    let mut uniswap_pools: Vec<_> = uniswap_pool_registry
        .pools()
        .keys()
        .map(|pool_id| {
            let initial_ticks_per_side = 200;
            EnhancedUniswapPool::new(
                DataLoader::new_with_registry(*pool_id, uniswap_pool_registry.clone()),
                initial_ticks_per_side
            )
        })
        .collect();

    for pool in uniswap_pools.iter_mut() {
        pool.initialize(Some(current_block), provider.clone())
            .await
            .unwrap();
    }

    let state_change_buffer = 100;
    UniswapPoolManager::new(
        uniswap_pools,
        current_block,
        state_change_buffer,
        Arc::new(CanonicalStateAdapter::new(state_notification))
    )
}
