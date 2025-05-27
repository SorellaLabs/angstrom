use std::{collections::HashSet, pin::Pin, sync::Arc};

use alloy::{self, eips::BlockId, network::Network, primitives::Address, providers::Provider};
use alloy_primitives::U256;
use angstrom::components::{StromHandles, init_network_builder};
use angstrom_eth::manager::EthEvent;
use angstrom_network::{PoolManagerBuilder, pool_manager::PoolHandle};
use angstrom_types::{
    block_sync::{BlockSyncProducer, GlobalBlockSync},
    consensus::ConsensusRoundName,
    contract_bindings::angstrom::Angstrom::PoolKey,
    contract_payloads::{
        CONFIG_STORE_SLOT, POOL_CONFIG_STORE_ENTRY_SIZE,
        angstrom::{
            AngPoolConfigEntry, AngstromPoolConfigStore, AngstromPoolPartialKey,
            UniswapAngstromRegistry
        }
    },
    primitive::{AngstromSigner, UniswapPoolRegistry},
    submission::SubmissionHandler
};
use consensus::{AngstromValidator, ConsensusManager, ManagerNetworkDeps};
use dashmap::DashMap;
use eyre::eyre;
use futures::{Stream, StreamExt, channel::mpsc::UnboundedReceiver};
use matching_engine::MatchingManager;
use order_pool::{PoolConfig, order_storage::OrderStorage};
use parking_lot::RwLock;
use reth::{providers::CanonStateSubscriptions, tasks::TaskExecutor};
use reth_provider::test_utils::TestCanonStateSubscriptions;
use telemetry::{NodeConstants, client::TelemetryClient};
use tokio_stream::wrappers::UnboundedReceiverStream;
use uniswap_v4::configure_uniswap_manager;
use validation::{common::TokenPriceGenerator, init_validation, validator::ValidationClient};

use crate::{providers::AnvilProvider, types::WithWalletProvider};

pub async fn load_poolconfig_from_chain<N, P>(
    angstrom_contract: Address,
    block_id: BlockId,
    provider: &P
) -> eyre::Result<DashMap<AngstromPoolPartialKey, AngPoolConfigEntry>>
where
    N: Network,
    P: Provider<N>
{
    // offset of 6 bytes
    let value = provider
        .get_storage_at(angstrom_contract, U256::from(CONFIG_STORE_SLOT))
        .block_id(block_id)
        .await
        .map_err(|e| eyre!("Error getting storage: {}", e))?;

    let value_bytes: [u8; 32] = value.to_be_bytes();
    tracing::debug!("storage slot of poolkey storage {:?}", value_bytes);
    let config_store_address = Address::from(<[u8; 20]>::try_from(&value_bytes[4..24]).unwrap());
    tracing::info!(?config_store_address);

    let code = provider
        .get_code_at(config_store_address)
        .block_id(block_id)
        .await
        .map_err(|e| eyre!("Error getting code: {}", e))?;

    tracing::info!(len=?code.len(), "bytecode: {:x}", code);

    if code.is_empty() {
        return Ok(DashMap::new());
    }

    if code.first() != Some(&0) {
        return Err(eyre!("Invalid encoded entries: must start with a safety byte of 0"));
    }
    tracing::info!(bytecode_len=?code.len());
    let adjusted_entries = &code[1..];
    if adjusted_entries.len() % POOL_CONFIG_STORE_ENTRY_SIZE != 0 {
        tracing::info!(bytecode_len=?adjusted_entries.len(), ?POOL_CONFIG_STORE_ENTRY_SIZE);
        return Err(eyre!("Invalid encoded entries: incorrect length after removing safety byte"));
    }
    let entries = adjusted_entries
        .chunks(POOL_CONFIG_STORE_ENTRY_SIZE)
        .enumerate()
        .map(|(index, chunk)| {
            let pool_partial_key =
                AngstromPoolPartialKey::new(<[u8; 27]>::try_from(&chunk[0..27]).unwrap());
            let tick_spacing = u16::from_be_bytes([chunk[27], chunk[28]]);
            let fee_in_e6 = u32::from_be_bytes([0, chunk[29], chunk[30], chunk[31]]);
            (
                pool_partial_key,
                AngPoolConfigEntry {
                    pool_partial_key,
                    tick_spacing,
                    fee_in_e6,
                    store_index: index
                }
            )
        })
        .collect();
    Ok(entries)
}

pub async fn initialize_strom_components_at_block<Provider: WithWalletProvider>(
    mut handles: StromHandles,
    telemetry_constants: NodeConstants,
    provider: AnvilProvider<Provider>,
    executor: TaskExecutor,
    pools: Vec<PoolKey>,
    block_id: u64
) -> eyre::Result<(PoolHandle, tokio::sync::mpsc::UnboundedReceiver<ConsensusRoundName>)> {
    // Get our URL

    // Constants that we want to work with
    let signer = AngstromSigner::for_address(telemetry_constants.node_address());
    let angstrom_contract = telemetry_constants.angstrom_address();
    let pool_manager = telemetry_constants.pool_manager_address();
    let deploy_block = telemetry_constants.angstrom_deploy_block();
    let gas_token = telemetry_constants.gas_token_address();
    let node_set = HashSet::from([telemetry_constants.node_address()]);
    let mock_canon = TestCanonStateSubscriptions::default();

    let network_builder = init_network_builder(
        signer.clone(),
        handles.eth_handle_rx.take().unwrap(),
        Arc::new(RwLock::new(node_set.clone()))
    )
    .unwrap();
    // Create our provider
    let submission_handler =
        SubmissionHandler::new(provider.rpc_provider().into(), &[], &[], &[], angstrom_contract);

    // EXTERNAL DATA - we can specify block by using a specific ID
    let pool_config_data = load_poolconfig_from_chain(
        angstrom_contract,
        BlockId::number(block_id),
        &provider.rpc_provider()
    )
    .await
    .unwrap();

    let pool_config = Arc::new(AngstromPoolConfigStore::from_entries(pool_config_data));

    // re-fetch given the fetch pools takes awhile. given this, we do techincally
    // have a gap in which a pool is deployed durning startup. This isn't
    // critical but we will want to fix this down the road.
    // let block_id = querying_provider.get_block_number().await.unwrap();

    // Data stream - can be mocked if we're looking at a single block
    let (_eth_event_tx, eth_event_rx) = tokio::sync::mpsc::unbounded_channel::<EthEvent>();
    let eth_event_rx_stream = UnboundedReceiverStream::new(eth_event_rx);
    let (_eth_event_tx_pmb, eth_event_rx_pmb) = tokio::sync::mpsc::unbounded_channel::<EthEvent>();
    let eth_event_rx_stream_pmb = UnboundedReceiverStream::new(eth_event_rx_pmb);
    // let eth_data_sub = node.provider.subscribe_to_canonical_state();

    let global_block_sync = GlobalBlockSync::new(block_id);

    // INTERNAL DATA
    let uniswap_registry: UniswapPoolRegistry = pools.into();
    let uni_ang_registry =
        UniswapAngstromRegistry::new(uniswap_registry.clone(), pool_config.clone());

    // Build our PoolManager using the PoolConfig and OrderStorage we've already
    // created
    let network_stream =
        Box::pin(eth_event_rx_stream) as Pin<Box<dyn Stream<Item = EthEvent> + Send + Sync>>;

    let telemetry: Option<TelemetryClient> = None;
    // Takes updates that are generally provided by EthDataCleanser
    let uniswap_pool_manager = configure_uniswap_manager(
        provider.rpc_provider().into(),
        mock_canon.subscribe_to_canonical_state(),
        uniswap_registry,
        block_id,
        global_block_sync.clone(),
        pool_manager,
        network_stream,
        telemetry.clone()
    )
    .await;

    tracing::info!("uniswap manager start");

    let uniswap_pools = uniswap_pool_manager.pools();
    let pool_ids = uniswap_pool_manager.pool_addresses().collect::<Vec<_>>();

    executor.spawn_critical("uniswap pool manager", Box::pin(uniswap_pool_manager));
    // EXTERNAL DATA - reads the price history from the chain to establish the price
    // background. Can be snapshotted or re-read from the chain
    let price_generator = TokenPriceGenerator::new(
        provider.rpc_provider().into(),
        block_id,
        uniswap_pools.clone(),
        gas_token,
        None
    )
    .await
    .expect("failed to start token price generator");

    // Needs to regularly talk to the chain, this one is complicated.  However,
    // given a single block and no actual updates to the canonical state we should
    // be able to freeze this in time at a specific block
    let update_stream = Box::pin(
        mock_canon
            .canonical_state_stream()
            .then(async |x| (0_u128, vec![]))
    );
    init_validation(
        provider.state_provider(),
        block_id,
        angstrom_contract,
        telemetry_constants.node_address(),
        // Because this is incapsulated under the orderpool syncer. this is the only case
        // we can use the raw stream.
        update_stream,
        uniswap_pools.clone(),
        price_generator,
        pool_config.clone(),
        handles.validator_rx
    );

    let validation_handle = ValidationClient(handles.validator_tx.clone());
    tracing::info!("validation manager start");

    let network_handle = network_builder
        .with_pool_manager(handles.pool_tx)
        .with_consensus_manager(handles.consensus_tx_op)
        .build_handle(executor.clone(), provider.rpc_provider());

    // fetch pool ids

    let pool_config = PoolConfig::with_pool_ids(pool_ids);
    let order_storage = Arc::new(OrderStorage::new(&pool_config));

    let pool_handle = PoolManagerBuilder::new(
        validation_handle.clone(),
        Some(order_storage.clone()),
        network_handle.clone(),
        eth_event_rx_stream_pmb,
        handles.pool_rx,
        global_block_sync.clone(),
        telemetry.clone()
    )
    .with_config(pool_config)
    .build_with_channels(
        executor.clone(),
        handles.orderpool_tx,
        handles.orderpool_rx,
        handles.pool_manager_tx,
        block_id
    );
    let validators = node_set
        .into_iter()
        // use same weight for all validators
        .map(|addr| AngstromValidator::new(addr, 100))
        .collect::<Vec<_>>();
    tracing::info!("pool manager start");

    // spinup matching engine
    let matching_handle = MatchingManager::spawn(executor.clone(), validation_handle.clone());

    let (state_tx, state_rx) = tokio::sync::mpsc::unbounded_channel();
    let manager = ConsensusManager::new(
        ManagerNetworkDeps::new(
            network_handle.clone(),
            mock_canon.subscribe_to_canonical_state(),
            handles.consensus_rx_op
        ),
        signer,
        validators,
        order_storage.clone(),
        deploy_block,
        block_id,
        uni_ang_registry,
        uniswap_pools.clone(),
        submission_handler,
        matching_handle,
        global_block_sync.clone(),
        handles.consensus_rx_rpc,
        telemetry,
        Some(state_tx)
    );

    executor.spawn_critical_with_graceful_shutdown_signal("consensus", move |grace| {
        manager.run_till_shutdown(grace)
    });

    global_block_sync.finalize_modules();
    tracing::info!("started angstrom");

    Ok((pool_handle, state_rx))
}
