//! Flashblocks subscriber.
//!
//! NOTE: This code was adapted from <https://github.com/base/node-reth/blob/main/crates/flashblocks-rpc/src/subscription.rs>
use std::{io::Read, sync::Arc};

use angstrom_types::flashblocks::{FlashblocksPayloadV1, PendingChain};
use futures::StreamExt;
use reth::{
    chainspec::EthChainSpec,
    providers::{BlockReaderIdExt, ChainSpecProvider, StateProviderFactory}
};
use reth_optimism_chainspec::OpHardforks;
use reth_primitives_traits::Header;
use tokio::sync::broadcast;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;

use crate::state::PendingStateWriter;

/// Subscribes to a Flashblocks websocket stream and notifies the state writer
/// of new flashblocks.
pub struct FlashblocksSubscriber<P: Clone> {
    ws:      Url,
    /// The current pending chain.
    pending: Option<PendingChain>,
    writer:  PendingStateWriter<P>
}

impl<P> FlashblocksSubscriber<P>
where
    P: StateProviderFactory
        + ChainSpecProvider<ChainSpec: EthChainSpec<Header = Header> + OpHardforks>
        + BlockReaderIdExt<Header = Header>
        + Clone
        + 'static
{
    pub fn new(ws: Url, writer: PendingStateWriter<P>) -> Self {
        Self { ws, pending: None, writer }
    }

    /// Spawns the subscriber.
    pub async fn start(self) {
        tracing::info!(ws = %self.ws, "Subscribing to Flashblocks ⚡");

        let ws = self.ws.clone();

        let mut backoff = std::time::Duration::from_secs(1);
        const MAX_BACKOFF: std::time::Duration = std::time::Duration::from_secs(10);

        loop {
            match connect_async(ws.as_str()).await {
                Ok((ws_stream, _)) => {
                    tracing::info!("WebSocket connection established");

                    let (_, mut read) = ws_stream.split();

                    while let Some(msg) = read.next().await {
                        match msg {
                            Ok(Message::Binary(bytes)) => match try_decode_message(&bytes) {
                                Ok(payload) => {
                                    if let Err(e) = self.writer.on_flashblock(payload) {
                                        tracing::error!(
                                            error = %e,
                                            "Error handling Flashblock"
                                        );
                                    }
                                }
                                Err(e) => {
                                    tracing::error!(
                                        message = "error decoding flashblock message",
                                        error = %e
                                    );
                                }
                            },
                            Ok(Message::Text(_)) => {
                                tracing::error!(
                                    "Received flashblock as plaintext, only compressed \
                                     flashblocks supported. Set up websocket-proxy to use \
                                     compressed flashblocks."
                                );
                            }
                            Ok(Message::Close(_)) => {
                                tracing::info!(message = "WebSocket connection closed by upstream");
                                break;
                            }
                            Err(e) => {
                                tracing::error!(
                                    message = "error receiving message",
                                    error = %e
                                );
                                break;
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(
                        message = "WebSocket connection error, retrying",
                        backoff_duration = ?backoff,
                        error = %e
                    );
                    tokio::time::sleep(backoff).await;
                    backoff = std::cmp::min(backoff * 2, MAX_BACKOFF);
                    continue;
                }
            }
        }
    }
}

fn try_decode_message(bytes: &[u8]) -> eyre::Result<FlashblocksPayloadV1> {
    let text = try_parse_message(bytes)?;

    serde_json::from_str(&text).map_err(|e| eyre::eyre!("failed to parse message: {}", e))
}

fn try_parse_message(bytes: &[u8]) -> eyre::Result<String> {
    if let Ok(text) = String::from_utf8(bytes.to_vec()) {
        if text.trim_start().starts_with("{") {
            return Ok(text);
        }
    }

    let mut decompressor = brotli::Decompressor::new(bytes, 4096);
    let mut decompressed = Vec::new();
    decompressor.read_to_end(&mut decompressed)?;

    let text = String::from_utf8(decompressed)?;
    Ok(text)
}
