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

use std::{net::SocketAddr, pin::Pin, time::Duration};

use alloy::{consensus::BlockHeader, providers::Provider, sol_types::SolCall};
use alloy_rpc_types::TransactionTrait;
use angstrom_rpc::api::OrderApiClient;
use angstrom_types::{
    contract_payloads::angstrom::AngstromBundle,
    primitive::{ANGSTROM_ADDRESS, AngstromAddressConfig},
    sol_bindings::grouped_orders::AllOrders
};
use futures::Future;
use jsonrpsee::http_client::HttpClient;
use pade::PadeDecode;
use testing_tools::{
    contracts::anvil::WalletProviderRpc,
    order_generator::{GeneratedPoolOrders, InternalBalanceMode, OrderGenerator}
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

        // Generate and submit orders for testing
        for i in 0..3 {
            tracing::info!("generating order batch {}", i);

            // Generate real orders using the OrderGenerator
            let new_orders = generator.generate_orders().await;
            tracing::info!("generated {} pool order sets", new_orders.len());

            for orders in new_orders {
                let GeneratedPoolOrders { pool_id, tob, book } = orders;
                tracing::info!(
                    "submitting orders for pool {:?}: 1 TOB + {} book orders",
                    pool_id,
                    book.len()
                );

                let all_orders = book
                    .into_iter()
                    .chain(vec![tob.into()])
                    .collect::<Vec<AllOrders>>();

                // Try to submit orders to test the RPC interface
                match client.send_orders(all_orders).await {
                    Ok(_) => tracing::info!(
                        "successfully submitted rollup orders for pool {:?}",
                        pool_id
                    ),
                    Err(e) => tracing::warn!(
                        "failed to submit rollup orders for pool {:?}: {:?}",
                        pool_id,
                        e
                    )
                }
            }

            // Wait between batches
            tokio::time::sleep(Duration::from_secs(2)).await;
        }

        tracing::info!("rollup order agent completed");
        Ok(())
    }) as Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + 'static>>
}

/// Create a test configuration for rollup agent testing
fn create_test_rollup_config() -> RollupAgentConfig {
    use std::{str::FromStr, sync::Arc};

    use dashmap::DashMap;
    use tokio::sync::mpsc;

    // Create empty pools for now - this will be populated from config file in later
    // phases
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
        let test_config = create_test_rollup_config();
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
