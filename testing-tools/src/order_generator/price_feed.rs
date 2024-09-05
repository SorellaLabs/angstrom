use std::{sync::Arc, task::Poll, time::Duration};

use futures::{stream::SplitStream, Stream, StreamExt};
use jsonrpsee::core::Serialize;
use serde::Deserialize;
use tokio::{
    net::TcpStream,
    sync::{broadcast, RwLock},
    time::{interval, Interval}
};
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

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

#[derive(Debug)]
pub struct PriceFeed {
    cache:   DepthUpdate,
    tcp:     SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    release: Interval
}

impl PriceFeed {
    pub async fn new(
        base_url: Option<String>,
        symbol: Option<String>,
        depth: Option<u32>,
        ex_interval: Option<String>,
        update_freq: Duration
    ) -> Self {
        let base_url = base_url.unwrap_or_else(|| "wss://stream.binance.com:443/ws".to_string());
        let symbol = symbol.unwrap_or_else(|| "ethusdc".to_string());
        let depth = depth.unwrap_or(5);
        let ex_interval = ex_interval.unwrap_or_else(|| "100ms".to_string());
        let url = format!("{}/{}@depth{}@{}", base_url, symbol, depth, ex_interval);
        let (ws_stream, _) = connect_async(&url).await.expect("Failed to connect");
        let (_, read) = ws_stream.split();
        Self {
            cache:   DepthUpdate {
                last_updated_id: 0,
                bids:            Vec::new(),
                asks:            Vec::new()
            },
            tcp:     read,
            release: interval(update_freq)
        }
    }
}

impl Stream for PriceFeed {
    type Item = DepthUpdate;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>
    ) -> std::task::Poll<Option<Self::Item>> {
        if self.release.poll_tick(cx).is_ready() {
            return Poll::Ready(Some(self.cache.clone()))
        }

        while let Poll::Ready(Some(message)) = self.tcp.poll_next_unpin(cx) {
            if let Ok(text) = message.unwrap().to_text() {
                match serde_json::from_str::<DepthUpdate>(text) {
                    Ok(depth_update) => {
                        tracing::trace!(
                            "price update best bid: {:?} best ask: {:?}",
                            depth_update.bids.first(),
                            depth_update.asks.first()
                        );
                        self.cache = depth_update.clone();
                    }
                    Err(e) => {
                        tracing::error!("Failed to parse depth update: {} text {}", e, text);
                    }
                }
            }
        }

        Poll::Pending
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
