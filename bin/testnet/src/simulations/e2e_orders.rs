use std::{pin::Pin, sync::Arc};

use angstrom_rpc::{api::OrderApiClient, impls::OrderApi};
use angstrom_types::{
    primitive::{ANGSTROM_DOMAIN, CHAIN_ID, ChainExt},
    sol_bindings::{RawPoolOrder, grouped_orders::AllOrders},
    testnet::InitialTestnetState
};
use futures::{Future, StreamExt, stream::FuturesUnordered};
use jsonrpsee::http_client::HttpClient;
use reth_provider::{CanonStateSubscriptions, test_utils::NoopProvider};
use reth_tasks::TaskExecutor;
use testing_tools::{
    agents::AgentConfig,
    controllers::enviroments::AngstromTestnet,
    order_generator::{GeneratedPoolOrders, InternalBalanceMode, OrderGenerator},
    types::{
        actions::WithAction, checked_actions::WithCheckedAction, checks::WithCheck,
        config::DevnetConfig
    }
};
use tracing::{Instrument, Level, debug, info, span};

use crate::cli::e2e_orders::End2EndOrdersCli;

pub async fn run_e2e_orders(executor: TaskExecutor, cli: End2EndOrdersCli) -> eyre::Result<()> {
    let config = cli.testnet_config.make_config()?;

    let agents = vec![end_to_end_agent];
    tracing::info!(?ANGSTROM_DOMAIN, ?CHAIN_ID, "spinning up e2e nodes for angstrom");

    // spawn testnet
    let testnet =
        AngstromTestnet::spawn_testnet(NoopProvider::default(), config, agents, executor.clone())
            .await?;
    tracing::info!("e2e testnet is alive");

    executor
        .spawn_critical_blocking("testnet", testnet.run_to_completion(executor.clone()))
        .await?;
    Ok(())
}

fn end_to_end_agent<'a>(
    t: &'a InitialTestnetState,
    agent_config: AgentConfig
) -> Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + 'a>> {
    Box::pin(async move {
        tracing::info!("starting e2e agent");

        let rpc_address = format!("http://{}", agent_config.rpc_address);
        let client = HttpClient::builder().build(rpc_address).unwrap();
        let mut generator = OrderGenerator::new(
            agent_config.uniswap_pools.clone(),
            agent_config.current_block,
            client.clone(),
            10..15,
            0.5..0.9,
            InternalBalanceMode::Never
        );

        let mut stream =
            agent_config
                .state_provider
                .canonical_state_stream()
                .map(|node| match node {
                    reth_provider::CanonStateNotification::Commit { new }
                    | reth_provider::CanonStateNotification::Reorg { new, .. } => new.tip_number()
                });

        t.ex.spawn(
            async move {
                tracing::info!("waiting for new block");
                let mut pending_orders = FuturesUnordered::new();

                loop {
                    tokio::select! {
                        Some(block_number) = stream.next() => {
                            generator.new_block(block_number);
                            let new_orders = generator.generate_orders().await;
                            tracing::info!("generated new orders. submitting to rpc");

                            for orders in new_orders {
                                let GeneratedPoolOrders { pool_id, tob, book } = orders;
                                let all_orders = book
                                    .into_iter()
                                    .chain(vec![tob.into()])
                                    .collect::<Vec<AllOrders>>();

                                 pending_orders.push(client.send_orders(all_orders));
                            }
                        }
                        Some(_resolved_order) = pending_orders.next() => {
                        }

                    }
                }
            }
            .instrument(span!(Level::ERROR, "order builder", ?agent_config.agent_id))
        );

        Ok(())
    }) as Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + 'a>>
}

#[cfg(test)]
pub mod test {

    use std::{sync::atomic::AtomicBool, time::Duration};

    use alloy::{
        consensus::BlockHeader,
        providers::{Provider, WalletProvider},
        sol_types::SolCall
    };
    use alloy_primitives::aliases::U24;
    use alloy_rpc_types::{BlockTransactionsKind, TransactionTrait};
    use angstrom_types::{
        contract_bindings::{
            angstrom::Angstrom::{configurePoolCall, unlockWithEmptyAttestationCall},
            controller_v_1::{
                self,
                ControllerV1::{self, removePoolCall}
            }
        },
        contract_payloads::angstrom::AngstromBundle,
        primitive::{ANGSTROM_ADDRESS, AngstromAddressConfig, AngstromSigner},
        sol_bindings::rpc_orders::AttestAngstromBlockEmpty
    };
    use futures::{FutureExt, StreamExt};
    use pade::PadeDecode;
    use reth_tasks::{TaskSpawner, TokioTaskExecutor};
    use testing_tools::{contracts::anvil::WalletProviderRpc, utils::noop_agent};
    use tokio::time::timeout;

    use super::*;
    use crate::cli::{init_tracing, testnet::TestnetCli};

    fn internal_balance_agent<'a>(
        _: &'a InitialTestnetState,
        agent_config: AgentConfig
    ) -> Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + 'a>> {
        Box::pin(async move {
            tracing::info!("starting internal balance agent");

            let rpc_address = format!("http://{}", agent_config.rpc_address);
            let client = HttpClient::builder().build(rpc_address).unwrap();
            let mut generator = OrderGenerator::new(
                agent_config.uniswap_pools.clone(),
                agent_config.current_block,
                client.clone(),
                10..20,
                0.8..0.9,
                InternalBalanceMode::Always
            );

            let mut stream = agent_config
                .state_provider
                .canonical_state_stream()
                .map(|node| match node {
                    reth_provider::CanonStateNotification::Commit { new }
                    | reth_provider::CanonStateNotification::Reorg { new, .. } => new.tip_number()
                });

            tokio::spawn(
                async move {
                    let rpc_address = format!("http://{}", agent_config.rpc_address);
                    let client = HttpClient::builder().build(rpc_address).unwrap();
                    tracing::info!("waiting for new block");
                    let mut pending_orders = FuturesUnordered::new();

                    loop {
                        tokio::select! {
                            Some(block_number) = stream.next() => {
                                generator.new_block(block_number);
                                let new_orders = generator.generate_orders().await;
                                tracing::info!("generated new internal balance orders. submitting to rpc");

                                for orders in new_orders {
                                    let GeneratedPoolOrders { pool_id, tob, book } = orders;
                                    let all_orders = book
                                        .into_iter().chain(vec![tob.into()])
                                        .collect::<Vec<AllOrders>>();

                                     pending_orders.push(client.send_orders(all_orders));
                                }
                            }
                            Some(resolved_order) = pending_orders.next() => {
                                tracing::info!("internal balance orders resolved");
                            }

                        }
                    }
                }
                .instrument(span!(Level::ERROR, "internal balance order builder", ?agent_config.agent_id))
            );

            Ok(())
        }) as Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + 'a>>
    }

    fn testing_end_to_end_agent<'a>(
        _: &'a InitialTestnetState,
        agent_config: AgentConfig
    ) -> Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + 'a>> {
        Box::pin(async move {
            tracing::info!("starting e2e agent");

            let rpc_address = format!("http://{}", agent_config.rpc_address);
            let client = HttpClient::builder().build(rpc_address).unwrap();
            let mut generator = OrderGenerator::new(
                agent_config.uniswap_pools.clone(),
                agent_config.current_block,
                client.clone(),
                10..20,
                0.8..0.9,
                InternalBalanceMode::Never
            );

            let mut stream = agent_config
                .state_provider
                .canonical_state_stream()
                .map(|node| match node {
                    reth_provider::CanonStateNotification::Commit { new }
                    | reth_provider::CanonStateNotification::Reorg { new, .. } => new.tip_number()
                });

            tokio::spawn(
                async move {
                    let rpc_address = format!("http://{}", agent_config.rpc_address);
                    let client = HttpClient::builder().build(rpc_address).unwrap();
                    tracing::info!("waiting for new block");
                    let mut pending_orders = FuturesUnordered::new();

                    loop {
                        tokio::select! {
                            Some(block_number) = stream.next() => {
                                generator.new_block(block_number);
                                let new_orders = generator.generate_orders().await;
                                tracing::info!("generated new orders. submitting to rpc");

                                for orders in new_orders {
                                    let GeneratedPoolOrders { pool_id, tob, book } = orders;
                                    let all_orders = book
                                        .into_iter().chain(vec![tob.into()])
                                        .collect::<Vec<AllOrders>>();

                                     pending_orders.push(client.send_orders(all_orders));
                                }
                            }
                            Some(resolved_order) = pending_orders.next() => {
                                tracing::info!("orders resolved");
                            }

                        }
                    }
                }
                .instrument(span!(Level::ERROR, "order builder", ?agent_config.agent_id))
            );

            Ok(())
        }) as Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + 'a>>
    }

    async fn run_testnet_with_validation<F, V>(
        agent_fn: F,
        test_name: &str,
        validation_fn: V,
        ctx: &TaskExecutor
    ) -> eyre::Result<()>
    where
        F: Fn(
                &InitialTestnetState,
                AgentConfig
            ) -> Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + '_>>
            + Send
            + Sync
            + Clone
            + 'static,
        V: Fn(WalletProviderRpc) -> Pin<Box<dyn Future<Output = ()> + Send>>
    {
        let config = TestnetCli {
            eth_fork_url: "wss://ethereum-rpc.publicnode.com".to_string(),
            ..Default::default()
        };

        let config = config.make_config().unwrap();
        let agents = vec![agent_fn];

        tracing::info!("spinning up e2e nodes for {}", test_name);

        // spawn testnet
        let testnet =
            AngstromTestnet::spawn_testnet(NoopProvider::default(), config, agents, ctx.clone())
                .await
                .expect("failed to start angstrom testnet");

        // grab provider so we can query from the chain later.
        let provider = testnet.node_provider(Some(1)).rpc_provider();

        let task = ctx.spawn_critical("testnet", testnet.run_to_completion(ctx.clone()).boxed());

        tracing::info!("waiting for valid block in {}", test_name);
        assert!(
            timeout(Duration::from_secs(60 * 5), validation_fn(provider))
                .await
                .is_ok()
        );
        task.abort();
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn test_internal_balances_land() {
        init_tracing(3);
        AngstromAddressConfig::INTERNAL_TESTNET.try_init();
        let runner = reth::CliRunner::try_default_runtime().unwrap();

        runner.run_command_until_exit(|ctx| async move {
            run_testnet_with_validation(
                internal_balance_agent,
                "internal balance testing",
                |provider| Box::pin(wait_for_internal_balance_block(provider)),
                &ctx.task_executor
            )
            .await
        });
    }

    #[test]
    #[serial_test::serial]
    fn testnet_lands_block() {
        init_tracing(3);
        AngstromAddressConfig::INTERNAL_TESTNET.try_init();
        let runner = reth::CliRunner::try_default_runtime().unwrap();

        runner.run_command_until_exit(|ctx| async move {
            run_testnet_with_validation(
                testing_end_to_end_agent,
                "angstrom",
                |provider| Box::pin(wait_for_valid_block(provider)),
                &ctx.task_executor
            )
            .await
        });
    }

    async fn wait_for_bundle_block<F>(provider: WalletProviderRpc, validator: F)
    where
        F: Fn(&AngstromBundle) -> bool
    {
        // Wait for a bundle that matches the validation criteria
        let mut sub = provider
            .subscribe_blocks()
            .await
            .expect("failed to subscribe to blocks");
        while let Ok(next) = sub.recv().await {
            let bn = next.number();
            let block = provider
                .get_block(alloy_rpc_types::BlockId::Number(bn.into()))
                .full()
                .await
                .unwrap()
                .unwrap();
            if block
                .transactions
                .into_transactions_vec()
                .into_iter()
                .filter(|tx| tx.to() == Some(*ANGSTROM_ADDRESS.get().unwrap()))
                .filter_map(|tx| {
                    let calldata = tx.input().to_vec();
                    let mut slice = calldata.as_slice();
                    let bytes = angstrom_types::contract_bindings::angstrom::Angstrom::executeCall::abi_decode(slice).unwrap().encoded.to_vec();

                    let mut slice = bytes.as_slice();
                    let data = &mut slice;
                    let bundle: AngstromBundle = PadeDecode::pade_decode(data, None).unwrap();

                    validator(&bundle).then_some(true)
                })
                .count()
                != 0
            {
                break;
            }
        }
    }

    async fn wait_for_internal_balance_block(provider: WalletProviderRpc) {
        wait_for_bundle_block(provider, |bundle| {
            // Check that we have both TOB and user orders
            if bundle.top_of_block_orders.is_empty() || bundle.user_orders.is_empty() {
                return false;
            }

            // Verify that all orders use internal balances
            let all_tob_internal = bundle
                .top_of_block_orders
                .iter()
                .all(|order| order.use_internal);
            let all_user_internal = bundle.user_orders.iter().all(|order| order.use_internal);

            if all_tob_internal && all_user_internal {
                tracing::info!(
                    "Found block with internal balance orders: TOB={}, User={}",
                    bundle.top_of_block_orders.len(),
                    bundle.user_orders.len()
                );
                true
            } else {
                false
            }
        })
        .await;
    }

    async fn wait_for_valid_block(provider: WalletProviderRpc) {
        wait_for_bundle_block(provider, |bundle| {
            // Check that we have both TOB and user orders
            !(bundle.top_of_block_orders.is_empty() || bundle.user_orders.is_empty())
        })
        .await;
    }

    static WORKED: AtomicBool = AtomicBool::new(false);

    #[test]
    #[serial_test::serial]
    fn test_remove_add_pool() {
        init_tracing(3);
        AngstromAddressConfig::INTERNAL_TESTNET.try_init();

        let runner = reth::CliRunner::try_default_runtime().unwrap();

        runner.run_command_until_exit(|ctx| async move {
            let config = TestnetCli {
                eth_fork_url: "wss://ethereum-rpc.publicnode.com".to_string(),
                ..Default::default()
            };

            let config = config.make_config().unwrap();

            let agents = vec![add_remove_agent];
            tracing::info!("spinning up e2e nodes for angstrom");

            // spawn testnet
            let testnet = AngstromTestnet::spawn_testnet(
                NoopProvider::default(),
                config,
                agents,
                ctx.task_executor.clone()
            )
            .await
            .expect("failed to start angstrom testnet");

            // grab provider so we can query from the chain later.
            let provider = testnet.node_provider(Some(0)).rpc_provider();
            let addresses = testnet.get_random_peer(vec![]).get_init_state().clone();

            let ex = ctx.task_executor.clone();
            let testnet_task = ctx.task_executor.spawn_critical(
                "testnet",
                Box::pin(async move {
                    testnet.run_to_completion(ex).await;
                    tracing::info!("testnet run to completion");
                })
            );

            tracing::info!("testnet configured");

            // remove the first configured pool
            let pk = addresses.pool_keys.first().unwrap();
            let addr = provider.signer_addresses().collect::<Vec<_>>()[0];
            let cnt = provider.get_transaction_count(addr).await.unwrap();

            let controller_instance = ControllerV1::new(addresses.controller_addr, provider);

            let _ = controller_instance
                .removePool(pk.currency0, pk.currency1)
                .nonce(cnt)
                .send()
                .await
                .unwrap()
                .watch()
                .await
                .unwrap();
            tracing::info!("removed pool \n\n\n\n\n\n");

            // wait some time to ensure that we can properly index the node being removed

            tokio::time::sleep(Duration::from_secs(12 * 3)).await;
            tracing::info!("slept, adding pool now \n\n\n\n\n\n\n\n\n\n");

            let _ = controller_instance
                .configurePool(pk.currency0, pk.currency1, 120, pk.fee, U24::ZERO, U24::ZERO)
                .nonce(cnt + 1)
                .send()
                .await
                .unwrap()
                .watch()
                .await
                .unwrap();
            tracing::info!("configured pool \n\n\n\n\n\n\n\n\n\n");
            // wait for the pool to be re-indexed.
            tokio::time::sleep(Duration::from_secs(12 * 3)).await;

            assert!(
                WORKED.load(std::sync::atomic::Ordering::SeqCst),
                "failed to properly remove and add pool"
            );

            testnet_task.abort();
            eyre::Ok(())
        });
    }

    fn add_remove_agent<'a>(
        init: &'a InitialTestnetState,
        agent_config: AgentConfig
    ) -> Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + 'a>> {
        Box::pin(async move {
            tracing::info!("starting add remove listener");
            // what we want to do is remove and then add back a pool. from this we want to
            // see the pools update to ensure that configure + remove
            // functionality works.
            tokio::spawn(
                async move {
                    let start_pool_len = agent_config.uniswap_pools.len();
                    let mut lower = false;
                    let mut higher = false;

                    loop {
                        let this_len = agent_config.uniswap_pools.len();

                        if this_len + 1 == start_pool_len {
                            tracing::info!("processed removed pool");
                            lower = true;
                        } else if lower && this_len == start_pool_len {
                            tracing::info!("processed added pool");
                            higher = true;
                            break;
                        }
                        tokio::time::sleep(Duration::from_secs(6)).await;
                    }

                    WORKED.store(true, std::sync::atomic::Ordering::SeqCst);
                    tracing::info!("add remove agent completed");
                }
                .instrument(span!(Level::ERROR, "order builder", ?agent_config.agent_id))
            );

            Ok(())
        }) as Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + 'a>>
    }
}
