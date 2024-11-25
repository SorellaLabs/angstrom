use std::sync::Arc;

use alloy::{
    network::Ethereum,
    primitives::address,
    providers::{Provider, ProviderBuilder, RootProvider, WsConnect},
    pubsub::PubSubFrontend
};
use testing_tools::{order_generator::ArbitrageGenerator, types::MockBlockSync};
use tokio::signal::unix::{signal, SignalKind};
use uniswap_v4::uniswap::{
    pool::EnhancedUniswapPool, pool_data_loader::DataLoader, pool_manager::UniswapPoolManager,
    pool_providers::provider_adapter::ProviderAdapter
};

#[tokio::main]
async fn main() -> eyre::Result<()> {
    std::env::set_var("RUST_LOG", "info");
    tracing_subscriber::fmt::init();
    let log_level = tracing::level_filters::LevelFilter::current();
    tracing::info!("Logging initialized at level: {}", log_level);
    let ticks_per_side = 1000;
    let pool_address = address!("88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640");
    let state_change_buffer = 1;
    let ws_endpoint = std::env::var("ETHEREUM_WS_ENDPOINT")?;
    let ws = WsConnect::new(ws_endpoint);
    let ws_provider: RootProvider<PubSubFrontend, Ethereum> =
        ProviderBuilder::default().on_ws(ws).await.unwrap();
    let block_number = ws_provider.get_block_number().await.unwrap();

    let ws_provider = Arc::new(ws_provider);
    let pool_provider = ProviderAdapter::new(ws_provider.clone());
    let mut pool = EnhancedUniswapPool::new(DataLoader::new(pool_address), ticks_per_side);
    pool.initialize(Some(block_number), ws_provider.clone())
        .await?;

    let pool_manager = UniswapPoolManager::new(
        vec![pool],
        block_number,
        state_change_buffer,
        Arc::new(pool_provider),
        MockBlockSync
    );

    let symbol = "ethusdc".to_string();
    let order_generator = ArbitrageGenerator::new(pool_address, pool_manager, symbol);

    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;

    tokio::select! {
        _ = sigterm.recv() => {},
        _ = sigint.recv() => {},
        _ = order_generator.monitor() => {},
    }

    tracing::info!("Shutting down gracefully");
    Ok(())
}
