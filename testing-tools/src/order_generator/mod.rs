use std::{sync::Arc, time::Duration};

use alloy_primitives::{address, Address, I256};
use futures::stream::StreamExt;
use matching_engine::cfmm::uniswap::{
    pool::EnhancedUniswapV3Pool, pool_manager::UniswapPoolManager
};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, RwLock};
use tokio_tungstenite::connect_async;

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
    cache:     Arc<RwLock<DepthUpdate>>,
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
            cache: Arc::new(RwLock::new(DepthUpdate {
                last_updated_id: 0,
                bids:            Vec::new(),
                asks:            Vec::new()
            })),
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
            cache: Arc::new(RwLock::new(DepthUpdate {
                last_updated_id: 0,
                bids:            Vec::new(),
                asks:            Vec::new()
            })),
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
                        let mut cache = self.cache.write().await;
                        *cache = depth_update.clone();

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

    async fn price_cache(&self) -> (Vec<PriceLevel>, Vec<PriceLevel>) {
        let cache = self.cache.read().await;
        (cache.bids.clone(), cache.asks.clone())
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
        let to_address = address!("DecafC0ffee15BadDecafC0ffee15BadDecafC0f");

        let (mut bids, mut asks) = self.binance_feed.price_cache().await;
        let best_bid = bids.pop();
        let best_ask = asks.pop();
        tracing::info!(
            "Best Bid on Binance: Price: {:.6} USDC, Quantity: {:.6} ETH | Best Ask on Binance: \
             Price: {:.6} USDC, Quantity: {:.6} ETH",
            best_bid.as_ref().map_or(0.0, |bid| bid.price),
            best_bid.as_ref().map_or(0.0, |bid| bid.quantity),
            best_ask.as_ref().map_or(0.0, |ask| ask.price),
            best_ask.as_ref().map_or(0.0, |ask| ask.quantity)
        );

        if let (Some(best_bid), Some(best_ask)) = (best_bid, best_ask) {
            let (ask_profit, ask_binance_amount, ask_uniswap_fill_price, ask_uniswap_amount) =
                self.try_sell_on_uniswap(&pool, &best_ask).await;
            let (bid_profit, bid_binance_amount, bid_uniswap_fill_price, bid_uniswap_amount) =
                self.try_buy_on_uniswap(&pool, &best_bid).await;

            tracing::info!(
                "Ask Profit: {:.2} USDC vs Bid Profit: {:.2} USDC | Ask Fill Price: {:.3} USDC vs \
                 Bid Fill Price: {:.3} USDC | Ask Amount: {:.6} ETH vs Bid Amount: {:.6} ETH",
                ask_profit,
                bid_profit,
                ask_uniswap_fill_price,
                bid_uniswap_fill_price,
                ask_uniswap_amount,
                bid_uniswap_amount
            );

            // Determine which trade to execute based on profit
            let (
                profit,
                trade_type,
                binance_price,
                binance_amount,
                uniswap_fill_price,
                uniswap_amount
            ) = if ask_profit > bid_profit {
                (
                    ask_profit,
                    "BUY",
                    best_ask.price,
                    ask_binance_amount,
                    ask_uniswap_fill_price,
                    ask_uniswap_amount
                )
            } else {
                (
                    bid_profit,
                    "SELL",
                    best_bid.price,
                    bid_binance_amount,
                    bid_uniswap_fill_price,
                    bid_uniswap_amount
                )
            };

            if profit > 0_f64 {
                tracing::info!(
                    "{} on Binance vs {} on Uniswap | Binance: Price: {:.2} USDC vs Uniswap: Fill \
                     Price: {:.2} USDC | Binance: Amount: {:.6} ETH vs Uniswap: Amount: {:.6} ETH \
                     | Profit: {:.2} USDC",
                    trade_type,
                    if trade_type == "SELL" { "BUY" } else { "SELL" },
                    binance_price,
                    uniswap_fill_price,
                    binance_amount,
                    uniswap_amount,
                    profit,
                );
            }
        }
    }

    async fn try_sell_on_uniswap(
        &self,
        pool: &EnhancedUniswapV3Pool,
        best_ask: &PriceLevel
    ) -> (f64, f64, f64, f64) {
        let eth = pool.token_b;
        let ask_amount = best_ask.quantity * 10_f64.powi(pool.token_b_decimals as i32);
        let ask_amount_in =
            I256::from_dec_str(&ask_amount.to_string().split(".").next().unwrap()).unwrap();

        let (ask_swap_amount_in, ask_swap_amount_out) =
            pool.simulate_swap(eth, ask_amount_in, None).unwrap();

        // ETH/USDC
        let ask_uniswap_fill_price =
            self.calculate_uniswap_fill_price(&pool, ask_swap_amount_in, ask_swap_amount_out);

        let ask_binance_amount = best_ask.quantity;
        let ask_uniswap_amount =
            ask_swap_amount_out.abs().as_u64() as f64 / 10_f64.powi(pool.token_b_decimals as i32);

        let ask_profit =
            (ask_uniswap_fill_price * ask_binance_amount) - (best_ask.price * ask_uniswap_amount);

        (ask_profit, ask_binance_amount, ask_uniswap_fill_price, ask_binance_amount)
    }

    async fn try_buy_on_uniswap(
        &self,
        pool: &EnhancedUniswapV3Pool,
        best_bid: &PriceLevel
    ) -> (f64, f64, f64, f64) {
        let bid_amount =
            best_bid.quantity * best_bid.price * 10_f64.powi(pool.token_a_decimals as i32);
        let bid_amount_in =
            I256::from_dec_str(&bid_amount.to_string().split(".").next().unwrap()).unwrap();
        let usdc = pool.token_a;

        let (bid_swap_amount_in, bid_swap_amount_out) =
            pool.simulate_swap(usdc, bid_amount_in, None).unwrap();

        // ETH/USDC
        let bid_uniswap_fill_price =
            self.calculate_uniswap_fill_price(&pool, bid_swap_amount_in, bid_swap_amount_out);

        let bid_binance_amount = bid_amount / 10_f64.powi(pool.token_b_decimals as i32);
        let bid_uniswap_amount =
            bid_swap_amount_out.abs().as_u64() as f64 / 10_f64.powi(pool.token_b_decimals as i32);

        let bid_profit =
            (best_bid.price * bid_binance_amount) - (bid_uniswap_fill_price * bid_uniswap_amount);

        (bid_profit, bid_binance_amount, bid_uniswap_fill_price, bid_uniswap_amount)
    }

    fn calculate_uniswap_fill_price(
        &self,
        pool: &EnhancedUniswapV3Pool,
        swap_amount_in: I256,
        swap_amount_out: I256
    ) -> f64 {
        // assumes b_dec > a_dec
        (swap_amount_in.abs().as_u64() as f64 / swap_amount_out.abs().as_u64() as f64)
            * 10f64.powi((pool.token_b_decimals - pool.token_a_decimals) as i32)
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
