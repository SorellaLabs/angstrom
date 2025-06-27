use std::{collections::HashSet, pin::pin, sync::Arc, time::Duration};

use accounting::WalletAccounting;
use alloy::providers::Provider;
use angstrom_rpc::{
    api::OrderApiClient,
    types::{OrderSubscriptionFilter, OrderSubscriptionKind}
};
use angstrom_types::primitive::ANGSTROM_DOMAIN;
use clap::Parser;
use futures::{StreamExt, TryStreamExt};
use itertools::Itertools;
use jsonrpsee::ws_client::{PingConfig, WsClientBuilder};
use order_manager::OrderManager;
use reth::tasks::TaskExecutor;
use sepolia_bundle_lander::{
    cli::{BundleLander, JsonPKs},
    env::BundleWashTraderEnv
};
use tracing::Level;
use tracing_subscriber::{filter, layer::SubscriberExt, util::SubscriberInitExt};

pub mod accounting;
pub mod order_manager;

#[inline]
pub fn run() -> eyre::Result<()> {
    let args = BundleLander::parse();
    reth::CliRunner::try_default_runtime()
        .unwrap()
        .run_command_until_exit(|ctx| start(args, ctx.task_executor))
}

pub async fn start(cfg: BundleLander, executor: TaskExecutor) -> eyre::Result<()> {
    init_tracing();

    let keys: JsonPKs = serde_json::from_str(&std::fs::read_to_string(&cfg.secret_keys_path)?)?;
    let env = BundleWashTraderEnv::init(&cfg, keys).await?;

    let domain = ANGSTROM_DOMAIN.get().unwrap();
    tracing::info!(?domain);

    executor
        .spawn_critical_with_graceful_shutdown_signal("counter_matcher", |mut signal| async move {
            let ws = Arc::new(
                WsClientBuilder::new()
                    .enable_ws_ping(PingConfig::default())
                    .build(cfg.node_endpoint.as_str())
                    .await
                    .expect("node endpoint must be WS")
            );

            let BundleWashTraderEnv { keys, provider, pools } = env;
            let all_tokens = pools
                .iter()
                .flat_map(|pool| [pool.token0, pool.token1])
                .unique()
                .collect::<Vec<_>>();

            let block_subscription = provider
                .clone()
                .watch_blocks()
                .await
                .unwrap()
                .with_poll_interval(Duration::from_millis(50))
                .into_stream()
                .flat_map(futures::stream::iter)
                .then(|hash| {
                    let provider_c = provider.clone();
                    async move { provider_c.get_block_by_hash(hash).hashes().await }
                });
            let mut block_sub = pin!(block_subscription);

            let Some(Ok(Some(current_block))) = block_sub.next().await else {
                tracing::error!("couldn't fetch next block");
                return;
            };
            // spawn up wallets

            let bn = current_block.header.number;
            let mut wallet_acc = Vec::new();
            for signer in keys {
                wallet_acc.push(
                    WalletAccounting::new(bn, signer, all_tokens.clone(), provider.clone()).await
                );
            }
            let mut order_manager = OrderManager::new(bn, provider.clone(), wallet_acc, ws.clone());

            let mut filters = HashSet::new();
            let mut subscriptions = HashSet::new();

            filters.insert(OrderSubscriptionFilter::None);
            subscriptions.insert(OrderSubscriptionKind::NewOrders);
            subscriptions.insert(OrderSubscriptionKind::FilledOrders);
            subscriptions.insert(OrderSubscriptionKind::CancelledOrders);
            subscriptions.insert(OrderSubscriptionKind::ExpiredOrders);

            let mut sub = ws
                .subscribe_orders(subscriptions, filters)
                .await
                .unwrap()
                .into_stream();
            loop {
                tokio::select! {
                    _ = &mut signal => {
                        tracing::info!("got shutdown");
                        break;
                    }
                     Some(Ok(event)) = sub.next() => {
                         order_manager.handle_event(event).await;
                    }
                     Some(Ok(Some(block))) = block_sub.next() => {
                         tracing::info!("new block");
                         order_manager.new_block(block.header.number).await;
                     }
                }
            }

            order_manager.shutdown().await;
        })
        .await?;
    Ok(())
}

fn init_tracing() {
    let level = Level::INFO;

    let envfilter = filter::EnvFilter::builder().try_from_env().ok();
    let format = tracing_subscriber::fmt::layer()
        .with_ansi(true)
        .with_target(true);

    if let Some(f) = envfilter {
        let _ = tracing_subscriber::registry()
            .with(format)
            .with(f)
            .try_init();
    } else {
        let filter = filter::Targets::new()
            .with_target("sepolia_bundle_lander", level)
            .with_target("angstrom_amm_quoter", level)
            .with_target("testnet", level)
            .with_target("devnet", level)
            .with_target("angstrom_rpc", level)
            .with_target("angstrom", level)
            .with_target("testing_tools", level)
            .with_target("counter_matcher", level)
            .with_target("angstrom_eth", level)
            .with_target("matching_engine", level)
            .with_target("uniswap_v4", level)
            .with_target("consensus", level)
            .with_target("validation", level)
            .with_target("order_pool", level);
        let _ = tracing_subscriber::registry()
            .with(format)
            .with(filter)
            .try_init();
    }
}
