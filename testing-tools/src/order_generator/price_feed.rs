use std::{sync::Arc, time::Duration};

use futures::StreamExt;
use jsonrpsee::core::Serialize;
use serde::Deserialize;
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
    pub price:    f64,
    #[serde(deserialize_with = "string_to_f64")]
    pub quantity: f64
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DepthUpdate {
    #[serde(rename = "lastUpdateId")]
    pub last_updated_id: u64,
    pub bids:            Vec<PriceLevel>,
    pub asks:            Vec<PriceLevel>
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
                        tracing::trace!(
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

    pub async fn price_cache(&self) -> (Vec<PriceLevel>, Vec<PriceLevel>) {
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
