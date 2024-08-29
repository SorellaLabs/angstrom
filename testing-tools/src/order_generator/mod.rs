use std::{sync::Arc, time::Duration};

use futures::StreamExt;
use jsonrpsee::core::__reexports::serde_json;
use matching_engine::cfmm::uniswap::pool_manager::UniswapPoolManager;
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{broadcast, RwLock},
    time
};
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
            symbol: symbol.unwrap_or_else(|| "ethusdt".to_string()),
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

        let mut interval = time::interval(Duration::from_secs(1));

        while let Some(message) = read.next().await {
            if let Ok(text) = message.unwrap().to_text() {
                match serde_json::from_str::<DepthUpdate>(text) {
                    Ok(depth_update) => {
                        tracing::debug!(
                            "price update bids: {:?} asks: {:?}",
                            depth_update.bids,
                            depth_update.asks
                        );
                        let mut bids = self.bids.write().await;
                        let mut asks = self.asks.write().await;
                        *bids = depth_update.bids.clone();
                        *asks = depth_update.asks.clone();
                        // intentionally throttled for testing
                        interval.tick().await;
                        self.update_tx
                            .send(depth_update)
                            .map_err(|_| PriceFeedError::UpdateSendError)?;
                    }
                    Err(e) => {
                        eprintln!("Failed to parse depth update: {}", e);
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
        let bids = self.bids.read().await;
        bids.clone()
    }

    async fn get_asks(&self) -> Vec<PriceLevel> {
        let asks = self.asks.read().await;
        asks.clone()
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
    binance_feed: PriceFeed,
    api_key: String,
    secret_key: String,
}

impl<P> OrderGenerator<P>
where
    P: matching_engine::cfmm::uniswap::pool_providers::PoolManagerProvider + Send + Sync + 'static
{
    pub async fn new(pool_manager: UniswapPoolManager<P>, binance_feed: PriceFeed, api_key: String, secret_key: String) -> Self {
        Self { pool_manager, binance_feed, api_key, secret_key }
    }

    pub async fn start(self) {
        let (mut pool_update_rx, _join_handles) =
            match self.pool_manager.subscribe_state_changes().await {
                Ok(result) => result,
                Err(e) => {
                    eprintln!("Failed to subscribe to state changes: {}", e);
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
        let pool_guard = self.pool_manager.pool().await;

        let uniswap_price = pool_guard.exchange_price(None).unwrap();
        let uniswap_quantity = pool_guard.exchange_quantity(None).unwrap();

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
        } else {
            tracing::info!(
                "Uniswap price: {}, quantity: {} | No Binance best bid/ask available",
                uniswap_price,
                uniswap_quantity
            );
        }

        if let (Some(best_bid), Some(best_ask)) = (best_bid, best_ask) {
            let binance_best_bid_price = best_bid.price;
            let binance_best_ask_price = best_ask.price;

            // Check for arbitrage opportunity
            if binance_best_bid_price > uniswap_price {
                // Generate order to sell on Binance and buy from the pool
                let amount = best_bid.quantity.min(uniswap_quantity as f64);
                self.binance_taker_order("SELL", amount).await;
                // TODO: place order on the pool
            } else if binance_best_ask_price < uniswap_price {
                // Generate order to buy on Binance and sell to the pool
                let amount = best_ask.quantity.min(uniswap_quantity as f64);
                self.binance_taker_order("BUY", amount).await;
                // TODO: place order on the pool
            }
        }
    }

    pub async fn binance_taker_order(&self, side: &str, quantity: f64) {
        let timestamp = chrono::Utc::now().timestamp_millis();
        let recv_window = 5000;

        let mut params = vec![
            ("symbol", "ETHUSDT"),
            ("side", side),
            ("type", "MARKET"),
            ("quantity", &quantity.to_string()),
            ("timestamp", &timestamp.to_string()),
            ("recvWindow", &recv_window.to_string()),
        ];

        let query_string = serde_urlencoded::to_string(&params).unwrap();
        let signature = hmac_sha256::HMAC::mac(query_string.as_bytes(), self.secret_key.as_bytes());
        let signature = hex::encode(signature);

        params.push(("signature", &signature));
        let url = format!("https://api.binance.com/api/v3/order?{}", serde_urlencoded::to_string(&params).unwrap());

        let client = reqwest::Client::new();
        let response = client
            .post(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await
            .unwrap();

        tracing::info!("Binance order response: {:?}", response.text().await);
    }

    pub fn uniswap_order(&self) {}
}
