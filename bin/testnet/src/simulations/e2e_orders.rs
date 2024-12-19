use std::pin::Pin;

use angstrom_eth::manager::ChainExt;
use angstrom_rpc::{api::OrderApiClient, impls::OrderApi};
use angstrom_types::{sol_bindings::grouped_orders::AllOrders, testnet::InitialTestnetState};
use futures::{stream::FuturesUnordered, Future, StreamExt};
use jsonrpsee::http_client::HttpClient;
use reth_provider::{test_utils::NoopProvider, CanonStateSubscriptions};
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
use tracing::{debug, info, span, Instrument, Level};

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
            0.5..0.6
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
