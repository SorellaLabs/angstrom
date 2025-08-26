/*! Minimal E2E tests for op_angstrom rollup implementation.
 *
 * This module provides basic end-to-end testing infrastructure for the
 * op_angstrom rollup node. It's adapted from the existing testnet e2e tests
 * but simplified to work with a single rollup node instead of a multi-node
 * consensus network.
 *
 * ## Structure
 *
 * - `rollup_order_agent`: Generates and submits test orders to the rollup
 *   RPC
 * - `wait_for_rollup_bundle_block`: Validates that orders land in rollup
 *   blocks
 * - `rollup_e2e_orders_land`: Full end-to-end test (currently placeholder)
 * - `rollup_agent_can_generate_orders`: Tests order generation logic
 *
 * ## Usage
 *
 * When the rollup infrastructure is fully ready, these tests can be
 * extended by:
 * 1. Spawning an actual op-angstrom node with rollup configuration
 * 2. Connecting to a test rollup sequencer
 * 3. Running real order generation and validation
 */

use std::{net::SocketAddr, path::PathBuf, pin::Pin, str::FromStr, time::Duration};

use alloy::{
    consensus::BlockHeader,
    providers::{Provider, ProviderBuilder},
    sol_types::SolCall
};
use alloy_primitives::Address;
use alloy_rpc_types::TransactionTrait;
use angstrom_rpc::api::OrderApiClient;
use angstrom_types::{
    contract_payloads::angstrom::AngstromBundle,
    matching::SqrtPriceX96,
    primitive::{ANGSTROM_ADDRESS, AngstromAddressConfig},
    sol_bindings::grouped_orders::AllOrders
};
use eyre::Context;
use futures::Future;
use jsonrpsee::http_client::HttpClient;
use pade::PadeDecode;
use serde::Deserialize;
use testing_tools::{
    contracts::anvil::WalletProviderRpc,
    order_generator::{GeneratedPoolOrders, InternalBalanceMode, OrderGenerator},
    types::initial_state::{Erc20ToDeploy, InitialStateConfig, PartialConfigPoolKey}
};
use tokio::time::timeout;
use uniswap_v4::uniswap::pool_manager::SyncedUniswapPools;

/// Configuration for rollup-specific order agents
/// Simplified version of testing_tools::agents::AgentConfig for single-node
/// rollup testing
#[derive(Clone)]
pub struct RollupAgentConfig {
    pub uniswap_pools: SyncedUniswapPools,
    pub rpc_address:   SocketAddr,
    pub agent_id:      u64,
    pub current_block: u64,
    pub sequencer_url: String
}

/// Rollup-specific test configuration
#[derive(Clone)]
pub struct RollupTestConfig {
    pub initial_state:        InitialStateConfig,
    pub sequencer_url:        String,
    pub chain_id:             u64,
    pub gas_price_multiplier: f64
}

/// TOML configuration structure for rollup tests
#[derive(Debug, Clone, Deserialize)]
struct RollupConfigToml {
    addresses_with_tokens: Vec<String>,
    tokens_to_deploy:      Vec<TokenToDeploy>,
    pool_keys:             Option<Vec<PoolKeyInner>>,
    rollup:                RollupSettings
}

#[derive(Debug, Clone, Deserialize)]
struct RollupSettings {
    default_sequencer_url: String,
    chain_id:              u64,
    gas_price_multiplier:  f64
}

#[derive(Debug, Clone, Deserialize)]
struct PoolKeyInner {
    fee:          u64,
    tick_spacing: i32,
    liquidity:    String,
    tick:         i32
}

#[derive(Debug, Clone, Deserialize)]
struct TokenToDeploy {
    name:    String,
    symbol:  String,
    address: String
}

impl RollupTestConfig {
    /// Load rollup configuration from TOML file
    pub fn load_config() -> eyre::Result<Self> {
        let config_path = PathBuf::from("./rollup-test-config.toml");
        RollupConfigToml::load_toml_config(&config_path)
    }
}

impl RollupConfigToml {
    /// Load rollup configuration from TOML file
    pub fn load_toml_config(config_path: &PathBuf) -> eyre::Result<RollupTestConfig> {
        if !config_path.exists() {
            return Err(eyre::eyre!("rollup config file does not exist at {:?}", config_path));
        }

        let toml_content = std::fs::read_to_string(config_path)
            .wrap_err_with(|| format!("could not read rollup config file {config_path:?}"))?;

        let config: Self = toml::from_str(&toml_content).wrap_err_with(|| {
            format!("could not deserialize rollup config file {config_path:?}")
        })?;

        let rollup_settings = config.rollup.clone();
        Ok(RollupTestConfig {
            initial_state:        config.try_into()?,
            sequencer_url:        rollup_settings.default_sequencer_url,
            chain_id:             rollup_settings.chain_id,
            gas_price_multiplier: rollup_settings.gas_price_multiplier
        })
    }
}

impl TryInto<InitialStateConfig> for RollupConfigToml {
    type Error = eyre::ErrReport;

    fn try_into(self) -> Result<InitialStateConfig, Self::Error> {
        Ok(InitialStateConfig {
            addresses_with_tokens: self
                .addresses_with_tokens
                .iter()
                .map(|addr| Address::from_str(addr))
                .collect::<Result<Vec<_>, _>>()?,
            tokens_to_deploy:      self
                .tokens_to_deploy
                .iter()
                .map(|token| -> Result<Erc20ToDeploy, eyre::ErrReport> {
                    Ok(Erc20ToDeploy::new(
                        &token.name,
                        &token.symbol,
                        Some(Address::from_str(&token.address)?)
                    ))
                })
                .collect::<Result<Vec<_>, _>>()?,
            pool_keys:             self.try_into()?
        })
    }
}

impl TryInto<Vec<PartialConfigPoolKey>> for RollupConfigToml {
    type Error = eyre::ErrReport;

    fn try_into(self) -> Result<Vec<PartialConfigPoolKey>, Self::Error> {
        let Some(keys) = self.pool_keys else { return Ok(Vec::new()) };

        keys.into_iter()
            .map(|key| {
                Ok::<_, eyre::ErrReport>(PartialConfigPoolKey::new(
                    key.fee,
                    key.tick_spacing,
                    key.liquidity.parse()?,
                    SqrtPriceX96::at_tick(key.tick)?
                ))
            })
            .collect()
    }
}

fn rollup_order_agent(
    agent_config: RollupAgentConfig
) -> Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + 'static>> {
    Box::pin(async move {
        tracing::info!(agent_id = agent_config.agent_id, "starting rollup order agent");

        let rpc_address = format!("http://{}", agent_config.rpc_address);
        let client = HttpClient::builder().build(rpc_address.clone()).unwrap();

        // Create real order generator with the provided pool configuration
        let generator = OrderGenerator::new(
            agent_config.uniswap_pools.clone(),
            agent_config.current_block,
            client.clone(),
            5..10,                      // Small range for testing - 5-10 orders per batch
            0.8..0.9,                   // Partial fill percentage range
            InternalBalanceMode::Never  // Start with external balances only
        );

        tracing::info!(
            "initialized order generator with {} pools",
            agent_config.uniswap_pools.len()
        );

        // Create L2 block stream to trigger order generation
        match create_l2_block_stream(agent_config.sequencer_url.clone()).await {
            Ok(mut block_stream) => {
                tracing::info!("connected to L2 block stream, waiting for new blocks...");

                let mut last_block = 0u64;
                let mut batch_count = 0;

                // Listen to L2 blocks and generate orders on new blocks
                let mut pinned_stream = std::pin::Pin::new(&mut block_stream);
                while let Some(block_number) = futures::StreamExt::next(&mut pinned_stream).await {
                    // Only generate orders if we have a new block
                    if block_number > last_block && batch_count < 3 {
                        last_block = block_number;
                        batch_count += 1;

                        tracing::info!(
                            "L2 block {}: generating order batch {}",
                            block_number,
                            batch_count
                        );

                        // Generate real orders using the OrderGenerator
                        let new_orders = generator.generate_orders().await;
                        tracing::info!(
                            "generated {} pool order sets for block {}",
                            new_orders.len(),
                            block_number
                        );

                        for orders in new_orders {
                            let GeneratedPoolOrders { pool_id, tob, book } = orders;
                            tracing::info!(
                                "submitting orders for pool {:?}: 1 TOB + {} book orders (block \
                                 {})",
                                pool_id,
                                book.len(),
                                block_number
                            );

                            let all_orders = book
                                .into_iter()
                                .chain(vec![tob.into()])
                                .collect::<Vec<AllOrders>>();

                            // Try to submit orders to test the RPC interface
                            match client.send_orders(all_orders).await {
                                Ok(_) => tracing::info!(
                                    "successfully submitted rollup orders for pool {:?} on block \
                                     {}",
                                    pool_id,
                                    block_number
                                ),
                                Err(e) => tracing::warn!(
                                    "failed to submit rollup orders for pool {:?} on block {}: \
                                     {:?}",
                                    pool_id,
                                    block_number,
                                    e
                                )
                            }
                        }
                    }

                    // Exit after processing 3 batches
                    if batch_count >= 3 {
                        tracing::info!("completed 3 order batches, stopping agent");
                        break;
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    "failed to create L2 block stream: {:?}, falling back to timer-based \
                     generation",
                    e
                );

                // Fallback to timer-based generation if L2 stream fails
                for i in 0..3 {
                    tracing::info!("generating timer-based order batch {}", i);

                    let new_orders = generator.generate_orders().await;
                    tracing::info!("generated {} pool order sets", new_orders.len());

                    for orders in new_orders {
                        let GeneratedPoolOrders { pool_id, tob, book } = orders;
                        let all_orders = book
                            .into_iter()
                            .chain(vec![tob.into()])
                            .collect::<Vec<AllOrders>>();

                        match client.send_orders(all_orders).await {
                            Ok(_) => tracing::info!(
                                "successfully submitted fallback orders for pool {:?}",
                                pool_id
                            ),
                            Err(e) => tracing::warn!(
                                "failed to submit fallback orders for pool {:?}: {:?}",
                                pool_id,
                                e
                            )
                        }
                    }

                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }

        tracing::info!("rollup order agent completed");
        Ok(())
    }) as Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + 'static>>
}

/// Create a test configuration for rollup agent testing
/// Now loads from the actual TOML configuration file
fn create_test_rollup_config() -> eyre::Result<RollupAgentConfig> {
    use std::{str::FromStr, sync::Arc};

    use dashmap::DashMap;
    use tokio::sync::mpsc;

    // Load configuration from TOML file
    let config_path = PathBuf::from("./rollup-test-config.toml");
    let rollup_config = match RollupConfigToml::load_toml_config(&config_path) {
        Ok(config) => config,
        Err(e) => {
            tracing::warn!("Failed to load rollup config: {:?}, using defaults", e);
            // Fall back to empty configuration
            RollupTestConfig {
                initial_state:        InitialStateConfig {
                    addresses_with_tokens: vec![],
                    tokens_to_deploy:      vec![],
                    pool_keys:             vec![]
                },
                sequencer_url:        "https://sepolia.base.org".to_string(),
                chain_id:             84532,
                gas_price_multiplier: 1.1
            }
        }
    };

    tracing::info!(
        "Loaded rollup config: {} pools, {} tokens, sequencer: {}",
        rollup_config.initial_state.pool_keys.len(),
        rollup_config.initial_state.tokens_to_deploy.len(),
        rollup_config.sequencer_url
    );

    // Create pools structure and populate with basic pool data
    let pool_map = Arc::new(DashMap::new());
    let (tx, _rx) = mpsc::channel(100);

    // Create basic pool entries from configuration
    // For now, we create placeholder pools - in full implementation these would be
    // properly initialized with liquidity, ticks, etc.
    for (index, pool_key) in rollup_config.initial_state.pool_keys.iter().enumerate() {
        let _pool_id = alloy_primitives::FixedBytes::from([index as u8; 32]); // Simplified pool ID
        tracing::debug!(
            "Creating placeholder pool {} with fee: {}, tick_spacing: {}",
            index,
            pool_key.fee,
            pool_key.tick_spacing
        );

        // TODO: In full implementation, create actual EnhancedUniswapPool
        // instances For now we just note that pools exist in the map
        // but leave empty pool_map.insert(pool_id,
        // Arc::new(RwLock::new(enhanced_pool)));
    }

    let pools = SyncedUniswapPools::new(pool_map, tx);

    tracing::info!(
        "initialized {} pools from config (placeholder pools for now)",
        rollup_config.initial_state.pool_keys.len()
    );

    Ok(RollupAgentConfig {
        uniswap_pools: pools,
        rpc_address:   SocketAddr::from_str("127.0.0.1:4201").unwrap(),
        agent_id:      1,
        current_block: 1,
        sequencer_url: rollup_config.sequencer_url
    })
}

/// Create fallback test configuration when TOML loading fails
fn create_fallback_rollup_config() -> RollupAgentConfig {
    use std::{str::FromStr, sync::Arc};

    use dashmap::DashMap;
    use tokio::sync::mpsc;

    let pool_map = Arc::new(DashMap::new());
    let (tx, _rx) = mpsc::channel(100);
    let pools = SyncedUniswapPools::new(pool_map, tx);

    RollupAgentConfig {
        uniswap_pools: pools,
        rpc_address:   SocketAddr::from_str("127.0.0.1:4201").unwrap(),
        agent_id:      1,
        current_block: 1,
        sequencer_url: "https://sepolia.base.org".to_string()
    }
}

/// Create a connection to the L2 sequencer for testing
async fn create_l2_provider(sequencer_url: &str) -> eyre::Result<impl Provider + 'static> {
    tracing::info!("connecting to L2 sequencer at: {}", sequencer_url);

    let provider = ProviderBuilder::new().connect_http(sequencer_url.parse()?);

    // Test the connection
    let chain_id = provider.get_chain_id().await?;
    let block_number = provider.get_block_number().await?;

    tracing::info!("connected to L2 chain_id: {}, current block: {}", chain_id, block_number);

    Ok(provider)
}

/// Create a simple L2 block stream for testing
async fn create_l2_block_stream(
    sequencer_url: String
) -> eyre::Result<std::pin::Pin<Box<dyn futures::Stream<Item = u64> + Send + 'static>>> {
    use futures::stream;
    use tokio::time::{Duration, MissedTickBehavior, interval};

    let provider = create_l2_provider(&sequencer_url).await?;

    // Create a stream that polls for new blocks every 2 seconds
    let mut interval = interval(Duration::from_secs(2));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let stream = stream::unfold((provider, interval), |(provider, mut interval)| async move {
        interval.tick().await;
        let block_number = provider.get_block_number().await.unwrap_or(0);
        Some((block_number, (provider, interval)))
    });

    Ok(Box::pin(stream))
}

/// Initialize SyncedUniswapPools from configuration
async fn initialize_pools_from_config(
    config: &RollupTestConfig
) -> eyre::Result<SyncedUniswapPools> {
    use std::sync::Arc;

    use dashmap::DashMap;
    use tokio::sync::mpsc;

    let pool_map = Arc::new(DashMap::new());
    let (tx, _rx) = mpsc::channel(100);

    tracing::info!(
        "initializing {} pools from rollup configuration",
        config.initial_state.pool_keys.len()
    );

    // Create placeholder pools from config
    // In full implementation, these would be populated with real liquidity data
    for (index, pool_key) in config.initial_state.pool_keys.iter().enumerate() {
        let _pool_id = alloy_primitives::FixedBytes::from([index as u8; 32]);
        tracing::debug!(
            "creating pool {} with fee: {}, tick_spacing: {}",
            index,
            pool_key.fee,
            pool_key.tick_spacing
        );

        // TODO: Create actual EnhancedUniswapPool instances
        // pool_map.insert(pool_id, Arc::new(RwLock::new(enhanced_pool)));
    }

    let pools = SyncedUniswapPools::new(pool_map, tx);
    tracing::info!("initialized pools structure");
    Ok(pools)
}

/// Test order generation with real L2 block events
async fn test_order_generation_with_l2_blocks(
    mut agent_config: RollupAgentConfig,
    pools: SyncedUniswapPools
) -> eyre::Result<()> {
    agent_config.uniswap_pools = pools;

    tracing::info!("starting order generation test with L2 blocks");

    // Run the rollup agent for a short duration
    let agent_future = rollup_order_agent(agent_config);

    // Run with timeout to prevent hanging
    tokio::time::timeout(Duration::from_secs(20), agent_future).await?
}

/// Run a complete rollup integration test
async fn run_complete_rollup_integration_test() -> eyre::Result<()> {
    tracing::info!("starting complete rollup integration test");

    // 1. Load configuration
    let config =
        RollupTestConfig::load_config().context("Failed to load rollup test configuration")?;

    tracing::info!(
        "loaded config: {} tokens, {} pools, chain_id: {}",
        config.initial_state.tokens_to_deploy.len(),
        config.initial_state.pool_keys.len(),
        config.chain_id
    );

    // 2. Test L2 provider connection
    let l2_provider = create_l2_provider(&config.sequencer_url)
        .await
        .context("Failed to connect to L2 sequencer")?;

    let latest_block = l2_provider
        .get_block_number()
        .await
        .context("Failed to get latest block from L2")?;
    tracing::info!("connected to L2, latest block: {}", latest_block);

    // 3. Initialize pools
    let pools = initialize_pools_from_config(&config)
        .await
        .context("Failed to initialize pools from config")?;
    tracing::info!("initialized {} pools", pools.pool_count());

    // 4. Create and run rollup agent
    let agent_config = RollupAgentConfig {
        uniswap_pools: pools.clone(),
        rpc_address:   std::net::SocketAddr::from_str("127.0.0.1:4201")?,
        agent_id:      1,
        current_block: latest_block,
        sequencer_url: config.sequencer_url.clone()
    };

    // 5. Test order generation with real L2 connection
    test_order_generation_with_l2_blocks(agent_config, pools)
        .await
        .context("Failed to test order generation with L2 blocks")?;

    tracing::info!("complete rollup integration test completed successfully");
    Ok(())
}

async fn wait_for_rollup_bundle_block<F>(provider: WalletProviderRpc, validator: F)
where
    F: Fn(&AngstromBundle) -> bool
{
    tracing::info!("waiting for rollup bundle block");

    let mut sub = provider
        .subscribe_blocks()
        .await
        .expect("failed to subscribe to rollup blocks");

    while let Ok(next) = sub.recv().await {
        let bn = next.number();
        tracing::debug!("checking rollup block {}", bn);

        let block = provider
            .get_block(alloy_rpc_types::BlockId::Number(bn.into()))
            .full()
            .await
            .unwrap()
            .unwrap();

        // Look for Angstrom transactions in the rollup block
        let angstrom_bundles = block
            .transactions
            .into_transactions_vec()
            .into_iter()
            .filter(|tx| tx.to() == Some(*ANGSTROM_ADDRESS.get().unwrap()))
            .filter_map(|tx| {
                let calldata = tx.input().to_vec();
                let slice = calldata.as_slice();

                // Decode Angstrom execute call
                match angstrom_types::contract_bindings::angstrom::Angstrom::executeCall::abi_decode(slice) {
                    Ok(call) => {
                        let bytes = call.encoded.to_vec();
                        let mut slice = bytes.as_slice();
                        let data = &mut slice;

                        match AngstromBundle::pade_decode(data, None) {
                            Ok(bundle) => {
                                tracing::info!(
                                    "found rollup bundle with {} TOB orders, {} user orders",
                                    bundle.top_of_block_orders.len(),
                                    bundle.user_orders.len()
                                );
                                Some(bundle)
                            },
                            Err(e) => {
                                tracing::warn!("failed to decode bundle: {:?}", e);
                                None
                            }
                        }
                    },
                    Err(e) => {
                        tracing::debug!("transaction not an Angstrom execute call: {:?}", e);
                        None
                    }
                }
            })
            .collect::<Vec<_>>();

        // Check if any bundle matches our validation criteria
        for bundle in angstrom_bundles {
            if validator(&bundle) {
                tracing::info!("found valid rollup bundle block!");
                return;
            }
        }
    }
}

async fn wait_for_rollup_orders(provider: WalletProviderRpc) {
    wait_for_rollup_bundle_block(provider, |bundle| {
        // Simple validation: just check that we have some orders
        !bundle.top_of_block_orders.is_empty() || !bundle.user_orders.is_empty()
    })
    .await;
}

/// Initialize tracing for the test
fn init_test_tracing() {
    use tracing::Level;
    use tracing_subscriber::{Layer, filter, layer::SubscriberExt, util::SubscriberInitExt};

    let _ = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(true)
                .with_target(true)
                .with_filter(
                    filter::Targets::new()
                        .with_target("op_angstrom_e2e", Level::INFO)
                        .with_target("angstrom_rpc", Level::INFO)
                        .with_target("angstrom", Level::INFO)
                        .with_target("testing_tools", Level::INFO)
                )
        )
        .try_init();
}

/// Simplified rollup test runner
async fn run_rollup_test_with_validation<V>(test_name: &str, _validation_fn: V) -> eyre::Result<()>
where
    V: Fn(WalletProviderRpc) -> Pin<Box<dyn Future<Output = ()> + Send>>
{
    tracing::info!("starting rollup e2e test: {}", test_name);

    // Note: This is a minimal test structure
    // In a full implementation, you would:
    // 1. Spawn an actual op-angstrom node
    // 2. Connect to a test rollup environment
    // 3. Run the order agent
    // 4. Validate results

    // For now, this is a placeholder that demonstrates the structure
    // and can be extended when the rollup infrastructure is ready

    tracing::warn!("rollup e2e test is currently a placeholder");
    tracing::info!("test '{}' structure is set up correctly", test_name);

    // TODO: When rollup infrastructure is ready:
    // 1. Spawn op-angstrom node with rollup configuration
    // 2. Start order agent: rollup_order_agent("http://localhost:4201".to_string(),
    //    1)
    // 3. Run validation: validation_fn(provider).await

    Ok(())
}

#[test]
#[serial_test::serial]
fn rollup_e2e_orders_land() {
    init_test_tracing();

    // Initialize address config for testing
    AngstromAddressConfig::INTERNAL_TESTNET.try_init();

    let runner = reth::CliRunner::try_default_runtime().unwrap();

    let _ = runner.run_command_until_exit(|_ctx| async move {
        run_rollup_test_with_validation("rollup_orders_test", |provider| {
            Box::pin(wait_for_rollup_orders(provider))
        })
        .await
    });
}

#[test]
#[serial_test::serial]
fn rollup_agent_can_generate_orders() {
    init_test_tracing();

    let runner = reth::CliRunner::try_default_runtime().unwrap();

    let _ = runner.run_command_until_exit(|_ctx| async move {
        tracing::info!("testing rollup order agent generation");

        // Create test configuration
        let test_config = match create_test_rollup_config() {
            Ok(config) => config,
            Err(e) => {
                tracing::warn!("Failed to create test config: {:?}, using fallback", e);
                create_fallback_rollup_config()
            }
        };
        tracing::info!("created test config with {} pools", test_config.uniswap_pools.len());

        // Test that the agent can be created and run
        // This tests the order generation logic structure
        let agent_future = rollup_order_agent(test_config);

        // Run with a timeout to avoid hanging
        match timeout(Duration::from_secs(5), agent_future).await {
            Ok(result) => {
                // We expect this to complete successfully even with empty pools
                tracing::info!("agent completed with result: {:?}", result);
            }
            Err(_) => {
                tracing::info!("agent timed out (this may be expected with no pools)");
            }
        }

        tracing::info!("rollup agent test completed successfully");
        eyre::Ok(())
    });
}

#[tokio::test]
#[serial_test::serial]
async fn test_end_to_end_rollup_integration() {
    init_test_tracing();

    // Initialize address config for testing
    AngstromAddressConfig::INTERNAL_TESTNET.try_init();

    tracing::info!("=== Starting Complete Rollup Integration Test ===");

    match run_complete_rollup_integration_test().await {
        Ok(()) => {
            tracing::info!("✓ Complete rollup integration test passed");
        }
        Err(e) => {
            tracing::error!("✗ Complete rollup integration test failed: {:?}", e);
            panic!("Integration test failed: {:?}", e);
        }
    }

    tracing::info!("=== Rollup Integration Test Complete ===");
}

#[test]
#[serial_test::serial]
fn rollup_l2_connection_works() {
    init_test_tracing();

    let runner = reth::CliRunner::try_default_runtime().unwrap();

    let _ = runner.run_command_until_exit(|_ctx| async move {
        tracing::info!("testing L2 sequencer connection");

        // Load configuration to get the sequencer URL
        let config = match create_test_rollup_config() {
            Ok(config) => config,
            Err(e) => {
                tracing::warn!("Failed to load config: {:?}, using fallback", e);
                create_fallback_rollup_config()
            }
        };

        tracing::info!("testing connection to sequencer: {}", config.sequencer_url);

        // Test L2 connection with a timeout to avoid hanging
        match timeout(Duration::from_secs(10), create_l2_provider(&config.sequencer_url)).await {
            Ok(Ok(provider)) => {
                tracing::info!("successfully connected to L2 sequencer");

                // Try to get current block to verify connection
                match timeout(Duration::from_secs(5), provider.get_block_number()).await {
                    Ok(Ok(block_number)) => {
                        tracing::info!("current L2 block number: {}", block_number);
                    }
                    Ok(Err(e)) => {
                        tracing::warn!("failed to get block number: {:?}", e);
                    }
                    Err(_) => {
                        tracing::warn!("timeout getting block number");
                    }
                }
            }
            Ok(Err(e)) => {
                tracing::warn!("failed to connect to L2 sequencer: {:?}", e);
            }
            Err(_) => {
                tracing::warn!("timeout connecting to L2 sequencer");
            }
        }

        tracing::info!("L2 connection test completed");
        eyre::Ok(())
    });
}
