use std::{path::PathBuf, pin::pin, sync::Arc, time::Duration};

use alloy::{primitives::Address, providers::Provider};
use alloy_rpc_types::TransactionTrait;
use angstrom_types::primitive::{ANGSTROM_ADDRESS, ANGSTROM_DOMAIN};
use futures::StreamExt;
use jsonrpsee::http_client::HttpClientBuilder;
use reth::tasks::TaskExecutor;
use serde::{Deserialize, Serialize};
use tracing::Level;
use tracing_subscriber::{filter, layer::SubscriberExt, util::SubscriberInitExt};
use url::Url;

use crate::{env::BundleWashTraderEnv, intent_builder::PoolIntentBundler};

#[derive(Debug, Clone, clap::Parser)]
pub struct BundleLander {
    /// angstrom endpoint
    #[clap(short, long, default_value = "http://localhost:8489")]
    pub node_endpoint:        Url,
    /// private keys to trade with
    /// can be either hex or byte vecs
    #[clap(short, long)]
    pub secret_keys_path:     PathBuf,
    /// address of angstrom
    #[clap(short, long)]
    pub angstrom_address:     Address,
    #[clap(short, long)]
    pub pool_manager_address: Address
}

/// the way that the bundle lander works is by more or less wash trading back
/// and forth on the sepolia testnet
impl BundleLander {
    pub async fn run(self, executor: TaskExecutor) -> eyre::Result<()> {
        init_tracing();

        let keys: JsonPKs =
            serde_json::from_str(&std::fs::read_to_string(&self.secret_keys_path)?)?;
        let env = BundleWashTraderEnv::init(&self, keys).await?;
        let domain = ANGSTROM_DOMAIN.get().unwrap();
        tracing::info!(?domain);
        tracing::info!("startup complete");

        executor
            .spawn_critical("order placer", async move {
                let BundleWashTraderEnv { keys, provider, pools } = env;

                let subscription = provider
                    .clone()
                    .watch_blocks()
                    .await
                    .unwrap()
                    .with_poll_interval(Duration::from_millis(50))
                    .into_stream()
                    .flat_map(futures::stream::iter)
                    .then(|hash| {
                        let provider_c = provider.clone();
                        async move { provider_c.get_block_by_hash(hash).full().await }
                    });

                let http_client =
                    Arc::new(HttpClientBuilder::new().build(self.node_endpoint).unwrap());

                let mut pinned = pin!(subscription);
                let Some(Ok(Some(current_block))) = pinned.next().await else {
                    tracing::error!("couldn't fetch next block");
                    return;
                };
                let mut processors = pools
                    .into_iter()
                    .map(|pool| {
                        PoolIntentBundler::new(
                            pool,
                            current_block.header.number,
                            keys.clone(),
                            provider.clone(),
                            http_client.clone()
                        )
                    })
                    .collect::<Vec<_>>();

                loop {
                    let new_block = pinned.next().await;
                    match new_block {
                        Some(Ok(Some(block))) => {
                            tracing::info!("new block");
                            // if we had and angstrom bundle, stop
                            let block_num = block.header.number;
                            if block
                                .into_transactions_vec()
                                .into_iter()
                                .any(|tx| tx.to() == Some(*ANGSTROM_ADDRESS.get().unwrap()))
                            {
                                tracing::info!("landed");
                                // break;
                            }
                            futures::stream::iter(&mut processors)
                                .for_each(|processor| async move {
                                    processor
                                        .new_block(block_num)
                                        .await
                                        .expect("failed to process new block in pool");
                                    processor
                                        .submit_new_orders_to_angstrom()
                                        .await
                                        .expect("failed to send angstrom orders");
                                })
                                .await;
                            tracing::info!(%block_num, "all orders submitted for block");
                        }
                        _ => {
                            tracing::error!("failed to get new block number");
                            break;
                        }
                    }
                }
            })
            .await?;

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonPKs {
    pub keys: Vec<String>
}

pub fn init_tracing() {
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
            .with_target("testnet", level)
            .with_target("devnet", level)
            .with_target("angstrom_rpc", level)
            .with_target("angstrom", level)
            .with_target("testing_tools", level)
            .with_target("angstrom_eth", level)
            .with_target("matching_engine", level)
            .with_target("uniswap_v4", level)
            .with_target("consensus", level)
            .with_target("angstrom_types_primitives", level)
            .with_target("angstrom_types_constants", level)
            .with_target("angstrom_rpc_api", level)
            .with_target("angstrom_rpc_types", level)
            .with_target("validation", level)
            .with_target("order_pool", level);
        let _ = tracing_subscriber::registry()
            .with(format)
            .with(filter)
            .try_init();
    }
}
