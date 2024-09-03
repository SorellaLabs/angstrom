use std::{sync::Arc, time::Duration};

use alloy_primitives::{address, I256};
use futures::stream::StreamExt;
use matching_engine::cfmm::uniswap::{
    pool::EnhancedUniswapV3Pool, pool_manager::UniswapPoolManager
};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, RwLock};
use tokio_tungstenite::connect_async;
use uniswap_v3_math::tick_math::MAX_SQRT_RATIO;

fn string_to_f64<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: serde::Deserializer<'de>
{
    let s: String = serde::Deserialize::deserialize(deserializer)?;
    s.parse::<f64>().map_err(serde::de::Error::custom)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PriceLevel {
    #[serde(deserialize_with = "string_to_f64")]
    price:    f64,
    #[serde(deserialize_with = "string_to_f64")]
    quantity: f64
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DepthUpdate {
    #[serde(rename = "lastUpdateId")]
    last_updated_id: u64,
    bids:            Vec<PriceLevel>,
    asks:            Vec<PriceLevel>
}

#[derive(Debug, Clone)]
pub struct PriceFeed {
    base_url:  String,
    symbol:    String,
    depth:     u32,
    interval:  String,
    bids:      Arc<RwLock<Vec<PriceLevel>>>,
    asks:      Arc<RwLock<Vec<PriceLevel>>>,
    update_tx: broadcast::Sender<DepthUpdate>
}

impl Default for PriceFeed {
    fn default() -> Self {
        let (update_tx, _) = broadcast::channel(100);
        Self {
            base_url: "wss://stream.binance.com:443/ws".to_string(),
            symbol: "ethusdc".to_string(),
            depth: 5,
            interval: "100ms".to_string(),
            bids: Arc::new(RwLock::new(Vec::new())),
            asks: Arc::new(RwLock::new(Vec::new())),
            update_tx
        }
    }
}

impl PriceFeed {
    pub fn new(
        base_url: Option<String>,
        symbol: Option<String>,
        depth: Option<u32>,
        interval: Option<String>
    ) -> Self {
        let (update_tx, _) = broadcast::channel(100);
        Self {
            base_url: base_url.unwrap_or_else(|| "wss://stream.binance.com:443/ws".to_string()),
            symbol: symbol.unwrap_or_else(|| "ethusdc".to_string()),
            depth: depth.unwrap_or(5),
            interval: interval.unwrap_or_else(|| "100ms".to_string()),
            bids: Arc::new(RwLock::new(Vec::new())),
            asks: Arc::new(RwLock::new(Vec::new())),
            update_tx
        }
    }

    fn get_url(&self) -> String {
        format!("{}/{}@depth{}@{}", self.base_url, self.symbol, self.depth, self.interval)
    }

    pub async fn start(&self) -> Result<(), PriceFeedError> {
        let url = self.get_url();
        let (ws_stream, _) = connect_async(&url).await.expect("Failed to connect");
        let (mut _write, mut read) = ws_stream.split();

        let mut last_update_time = tokio::time::Instant::now();

        while let Some(message) = read.next().await {
            if let Ok(text) = message.unwrap().to_text() {
                match serde_json::from_str::<DepthUpdate>(text) {
                    Ok(depth_update) => {
                        tracing::debug!(
                            "price update best bid: {:?} best ask: {:?}",
                            depth_update.bids.first(),
                            depth_update.asks.first()
                        );
                        let mut bids = self.bids.write().await;
                        let mut asks = self.asks.write().await;
                        *bids = depth_update.bids.clone();
                        *asks = depth_update.asks.clone();

                        // intentionally throttled for testing
                        if last_update_time.elapsed() >= Duration::from_secs(1) {
                            self.update_tx
                                .send(depth_update)
                                .map_err(|_| PriceFeedError::UpdateSendError)?;
                            last_update_time = tokio::time::Instant::now();
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to parse depth update: {} text {}", e, text);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DepthUpdate> {
        self.update_tx.subscribe()
    }

    async fn get_bids(&self) -> Vec<PriceLevel> {
        self.bids.read().await.clone()
    }

    async fn get_asks(&self) -> Vec<PriceLevel> {
        self.asks.read().await.clone()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PriceFeedError {
    #[error("Synchronization has already been started")]
    SyncAlreadyStarted,
    #[error("Synchronization has not been started")]
    SyncNotStarted,
    #[error("Failed to send update")]
    UpdateSendError
}

pub struct OrderGenerator<P> {
    pool_manager: UniswapPoolManager<P>,
    binance_feed: PriceFeed
}

impl<P> OrderGenerator<P>
where
    P: matching_engine::cfmm::uniswap::pool_providers::PoolManagerProvider + Send + Sync + 'static
{
    pub async fn new(pool_manager: UniswapPoolManager<P>, binance_feed: PriceFeed) -> Self {
        Self { pool_manager, binance_feed }
    }

    pub async fn start(self) {
        let (mut pool_update_rx, _join_handles) =
            match self.pool_manager.subscribe_state_changes().await {
                Ok(result) => result,
                Err(e) => {
                    tracing::error!("Failed to subscribe to state changes: {}", e);
                    return;
                }
            };

        let mut price_feed_rx = self.binance_feed.subscribe();

        loop {
            tokio::select! {
                Some((_address, _block_number)) = pool_update_rx.recv() => {
                    self.check_arbitrage().await;
                }
                Ok(depth_update) = price_feed_rx.recv() => {
                    tracing::debug!("Received depth update: {:?}", depth_update);
                    self.check_arbitrage().await;
                }
            }
        }
    }

    async fn check_arbitrage(&self) {
        // do it once at the top
        let pool = self.pool_manager.pool().await;
        let uniswap_price = pool.exchange_price(None).unwrap();
        let uniswap_quantity = pool.exchange_quantity(None).unwrap();

        let uniswap_quantity = uniswap_quantity as f64 / 10_f64.powi(pool.token_b_decimals as i32);
        let bids = self.binance_feed.get_bids().await;
        let asks = self.binance_feed.get_asks().await;
        let best_bid = bids.first();
        let best_ask = asks.first();
        if let (Some(best_bid), Some(best_ask)) = (best_bid, best_ask) {
            tracing::info!(
                "Uniswap price: {}, quantity: {} | Binance best bid: {}, quantity: {} | Binance \
                 best ask: {}, quantity: {}",
                uniswap_price,
                uniswap_quantity,
                best_bid.price,
                best_bid.quantity,
                best_ask.price,
                best_ask.quantity
            );
        }
        let to_address = address!("DecafC0ffee15BadDecafC0ffee15BadDecafC0f");
        if let (Some(best_bid), Some(best_ask)) = (best_bid, best_ask) {
            let binance_best_bid_price = best_bid.price;
            let binance_best_ask_price = best_ask.price;

            if binance_best_bid_price > uniswap_price {
                let amount = best_bid.quantity;
                let eth = pool.token_b;
                let amount = amount * 10_f64.powi(pool.token_b_decimals as i32);
                let amount_in = I256::from_dec_str(&amount.to_string()).unwrap();
                let (swap_amount_in, swap_amount_out) =
                    pool.simulate_swap(eth, amount_in, None).unwrap();
                let _call_data = pool.swap_calldata(
                    to_address,
                    eth == pool.token_a,
                    amount_in,
                    MAX_SQRT_RATIO,
                    vec![]
                );
                let uniswap_fill_price =
                    self.calculate_uniswap_fill_price(&pool, swap_amount_in, swap_amount_out);
                let binance_amount = amount / 10_f64.powi(pool.token_b_decimals as i32);
                let uniswap_amount = swap_amount_out.abs().as_u64() as f64
                    / 10_f64.powi(pool.token_b_decimals as i32);
                let profit = (binance_best_bid_price * binance_amount)
                    - (uniswap_fill_price * uniswap_amount);
                tracing::info!(
                    "SELL on Binance, BUY on Uniswap | Binance: Price: {:.2} USDC, Amount: {:.2} \
                     ETH | Uniswap: Price: {:.2} USDC, Amount: {:.2} ETH, Fill Price: {:.2} USDC \
                     | Profit: {:.2} USDC",
                    binance_best_bid_price,
                    binance_amount,
                    uniswap_price,
                    uniswap_amount,
                    uniswap_fill_price,
                    profit
                );
            } else if binance_best_ask_price < uniswap_price {
                let amount = best_ask.quantity;
                let eth = pool.token_b;
                let amount = amount * 10_f64.powi(pool.token_b_decimals as i32);
                let amount_in = I256::from_dec_str(&amount.to_string()).unwrap();
                let (swap_amount_in, swap_amount_out) =
                    pool.simulate_swap(eth, amount_in, None).unwrap();

                let _call_data = pool.swap_calldata(
                    to_address,
                    eth == pool.token_b,
                    amount_in,
                    MAX_SQRT_RATIO,
                    vec![]
                );

                let uniswap_fill_price =
                    self.calculate_uniswap_fill_price(&pool, swap_amount_in, swap_amount_out);
                let binance_amount = amount / 10_f64.powi(pool.token_b_decimals as i32);
                let uniswap_amount = swap_amount_out.abs().as_u64() as f64
                    / 10_f64.powi(pool.token_b_decimals as i32);
                let profit = (uniswap_fill_price * uniswap_amount)
                    - (binance_best_ask_price * binance_amount);
                tracing::info!(
                    "BUY on Binance, SELL on Uniswap | Binance: Price: {:.2} USDC, Amount: {:.2} \
                     ETH | Uniswap: Price: {:.2} USDC, Amount: {:.2} ETH, Fill Price: {:.2} USDC \
                     | Profit: {:.2} USDC",
                    binance_best_ask_price,
                    binance_amount,
                    uniswap_price,
                    uniswap_amount,
                    uniswap_fill_price,
                    profit
                );
            }
        }
    }

    fn calculate_uniswap_fill_price(
        &self,
        pool: &EnhancedUniswapV3Pool,
        swap_amount_in: I256,
        swap_amount_out: I256
    ) -> f64 {
        (swap_amount_in.abs().as_u64() as f64 / 10_f64.powi(pool.token_a_decimals as i32))
            / (swap_amount_out.abs().as_u64() as f64 / 10_f64.powi(pool.token_b_decimals as i32))
    }
}

// fn sqrt96_to_price(sqrt96_price: U256) -> f64 {
//     let sqrt_price = sqrt96_price.as_u64() as f64 / 2f64.powi(96);
//     sqrt_price * sqrt_price
// }
//
// fn price_to_sqrt96(price: f64) -> U256 {
//     let sqrt_price = price.sqrt();
//     let sqrt96_price = sqrt_price * 2f64.powi(96);
//     U256::from(sqrt96_price as u128)
// }
