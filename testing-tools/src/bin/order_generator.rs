use std::sync::Arc;

use alloy::{
    network::Ethereum,
    primitives::address,
    providers::{ProviderBuilder, RootProvider, WsConnect},
    pubsub::PubSubFrontend
};
use alloy_provider::Provider;
use matching_engine::cfmm::uniswap::{
    pool::EnhancedUniswapV3Pool, pool_manager::UniswapPoolManager,
    pool_providers::provider_adapter::ProviderAdapter
};
use testing_tools::order_generator::{price_feed::PriceFeed, OrderGenerator};
use tokio::signal::unix::{signal, SignalKind};

#[tokio::main]
async fn main() -> eyre::Result<()> {
    std::env::set_var("RUST_LOG", "testing_tools=debug,matching_engine=debug,info");
    tracing_subscriber::fmt::init();
    let log_level = tracing::level_filters::LevelFilter::current();
    tracing::info!("Logging initialized at level: {}", log_level);
    let ticks_per_side = 1000;
    let address = address!("88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640");
    let state_change_buffer = 1;
    let ws_endpoint = std::env::var("ETHEREUM_WS_ENDPOINT")?;
    let ws = WsConnect::new(ws_endpoint);
    let ws_provider: RootProvider<PubSubFrontend, Ethereum> =
        ProviderBuilder::default().on_ws(ws).await.unwrap();
    let block_number = ws_provider.get_block_number().await.unwrap();

    let ws_provider = Arc::new(ws_provider);
    let pool_provider = ProviderAdapter::new(ws_provider.clone());
    let mut pool = EnhancedUniswapV3Pool::new(address, ticks_per_side);
    // pool.set_sim_swap_sync(true);
    pool.initialize(Some(block_number), ws_provider.clone())
        .await?;
    pool.sync_ticks(Some(block_number), ws_provider.clone())
        .await?;
    let pool_manager =
        UniswapPoolManager::new(pool, block_number, state_change_buffer, Arc::new(pool_provider));

    let binance_feed = PriceFeed::default();
    let order_generator =
        OrderGenerator::new(pool_manager, binance_feed.clone(), ws_provider.clone()).await;

    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;

    tokio::select! {
        _ = sigterm.recv() => {},
        _ = sigint.recv() => {},
        _ = binance_feed.start() => {},
        _ = order_generator.start() => {},
    }

    tracing::info!("Shutting down gracefully");
    Ok(())
}
