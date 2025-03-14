use std::pin::Pin;

use angstrom_eth::manager::ChainExt;
use angstrom_rpc::{api::OrderApiClient, impls::OrderApi};
use angstrom_types::{sol_bindings::grouped_orders::AllOrders, testnet::InitialTestnetState};
use futures::{Future, StreamExt, stream::FuturesUnordered};
use jsonrpsee::http_client::HttpClient;
use reth_provider::{CanonStateSubscriptions, test_utils::NoopProvider};
use reth_tasks::TaskExecutor;
use testing_tools::{
    agents::AgentConfig,
    controllers::enviroments::AngstromTestnet,
    order_generator::{GeneratedPoolOrders, OrderGenerator},
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
    tracing::info!("spinning up e2e nodes for angstrom");

    // spawn testnet
    let testnet = AngstromTestnet::spawn_testnet(NoopProvider::default(), config, agents).await?;
    tracing::info!("e2e testnet is alive");

    executor
        .spawn_critical_blocking("testnet", testnet.run_to_completion(executor.clone()))
        .await?;
    Ok(())
}

fn end_to_end_agent<'a>(
    _: &'a InitialTestnetState,
    agent_config: AgentConfig
) -> Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + 'a>> {
    Box::pin(async move {
        tracing::info!("starting e2e agent");
        let mut generator = OrderGenerator::new(
            agent_config.uniswap_pools.clone(),
            agent_config.current_block,
            5..10,
            0.7..0.9
        );

        let mut stream =
            agent_config
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
                            let new_orders = generator.generate_orders();
                            tracing::info!("generated new orders. submitting to rpc");

                            for orders in new_orders {
                                let GeneratedPoolOrders { pool_id, tob, book } = orders;
                                let all_orders = book
                                    .into_iter()
                                    .map(Into::into)
                                    .chain(vec![tob.into()])
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

#[cfg(test)]
pub mod test {

    use std::time::Duration;

    use alloy::{consensus::BlockHeader, providers::Provider};
    use alloy_rpc_types::{BlockTransactionsKind, TransactionTrait};
    use angstrom_types::{
        contract_payloads::angstrom::AngstromBundle, primitive::TESTNET_ANGSTROM_ADDRESS
    };
    use futures::{FutureExt, StreamExt};
    use pade::PadeDecode;
    use reth_tasks::{TaskSpawner, TokioTaskExecutor};
    use testing_tools::contracts::anvil::WalletProviderRpc;
    use tokio::time::timeout;

    use super::*;
    use crate::cli::{init_tracing, testnet::TestnetCli};

    #[tokio::test(flavor = "multi_thread")]
    async fn testnet_lands_block() {
        init_tracing(4);
        let config = TestnetCli {
            eth_fork_url: "wss://ethereum-rpc.publicnode.com".to_string(),
            ..Default::default()
        };

        let config = config.make_config().unwrap();

        let agents = vec![end_to_end_agent];
        tracing::info!("spinning up e2e nodes for angstrom");

        // spawn testnet
        let testnet = AngstromTestnet::spawn_testnet(NoopProvider::default(), config, agents)
            .await
            .expect("failed to start angstrom testnet");

        // grab provider so we can query from the chain later.
        let provider = testnet.node_provider(Some(1)).rpc_provider();

        let executor = TokioTaskExecutor::default();
        let task =
            executor.spawn_critical("testnet", testnet.run_to_completion(executor.clone()).boxed());

        tracing::info!("waiting for valid block");
        assert!(
            timeout(Duration::from_secs(60 * 3), wait_for_valid_block(provider))
                .await
                .is_ok()
        );
        task.abort();
    }

    async fn wait_for_valid_block(provider: WalletProviderRpc) {
        // once started, it might take 2-3 blocks for a bundle with user orders to land.
        // we want to verify that both tob + user orders land.
        let mut sub = provider
            .subscribe_blocks()
            .await
            .expect("failed to subscribe to blocks");
        while let Ok(next) = sub.recv().await {
            let bn = next.number();
            let block = provider
                .get_block(alloy_rpc_types::BlockId::Number(bn.into()), BlockTransactionsKind::Full)
                .await
                .unwrap()
                .unwrap();
            if block
                .transactions
                .into_transactions_vec()
                .into_iter()
                .filter(|tx| tx.to() == Some(TESTNET_ANGSTROM_ADDRESS))
                .filter_map(|tx| {
                    let calldata = tx.input().to_vec();
                    let mut slice = calldata.as_slice();
                    let data = &mut slice;
                    let bundle: AngstromBundle = PadeDecode::pade_decode(data, None).unwrap();
                    (!(bundle.top_of_block_orders.is_empty() || bundle.user_orders.is_empty()))
                        .then_some(true)
                })
                .count()
                != 0
            {
                break;
            }
        }
    }
}
