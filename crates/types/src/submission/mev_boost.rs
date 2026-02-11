use std::{
    fmt::Debug,
    sync::Arc,
    task::{Context, Poll}
};

use alloy::{
    eips::Encodable2718,
    hex,
    primitives::keccak256,
    providers::{Provider, RootProvider},
    rpc::{
        client::ClientBuilder,
        json_rpc::{RequestPacket, ResponsePacket}
    },
    signers::Signer,
    transports::{TransportError, TransportErrorKind, TransportFut}
};
use alloy_primitives::Address;
use futures::stream::{StreamExt, iter};
use itertools::Itertools;
use reth::rpc::types::mev::{EthBundleHash, EthSendBundle};

use super::{
    AngstromBundle, AngstromSigner, ChainSubmitter, DEFAULT_SUBMISSION_CONCURRENCY,
    SubmissionResult, TxFeatureInfo, Url
};
use crate::primitive::AngstromMetaSigner;

pub struct MevBoostSubmitter {
    clients:          Vec<(RootProvider, Url)>,
    angstrom_address: Address
}

impl MevBoostSubmitter {
    pub fn new<S: AngstromMetaSigner>(
        urls: &[Url],
        signer: AngstromSigner<S>,
        angstrom_address: Address
    ) -> Self {
        let clients = urls
            .iter()
            .map(|url| {
                let transport = MevHttp::new_flashbots(url.clone(), (*signer).clone());
                let client = ClientBuilder::default().transport(transport, false);
                (RootProvider::new(client), url.clone())
            })
            .collect_vec();

        Self { clients, angstrom_address }
    }
}

impl ChainSubmitter for MevBoostSubmitter {
    fn angstrom_address(&self) -> Address {
        self.angstrom_address
    }

    fn submitter_type(&self) -> &'static str {
        "mev_boost"
    }

    fn submit<'a, S: AngstromMetaSigner>(
        &'a self,
        signer: &'a AngstromSigner<S>,
        bundle: Option<&'a AngstromBundle>,
        tx_features: &'a TxFeatureInfo
    ) -> std::pin::Pin<Box<dyn Future<Output = eyre::Result<Option<SubmissionResult>>> + Send + 'a>>
    {
        Box::pin(async move {
            let start = std::time::Instant::now();
            let bundle = bundle.ok_or_else(|| eyre::eyre!("no bundle was past in"))?;

            let tx = self
                .build_and_sign_tx_with_gas(signer, bundle, tx_features)
                .await;

            let hash = *tx.tx_hash();

            let bundle = EthSendBundle {
                txs: vec![tx.encoded_2718().into()],
                block_number: tx_features.target_block,
                ..Default::default()
            };

            // Clone here is fine as its in a Arc
            let results: Vec<_> = iter(self.clients.clone())
                .map(async |(client, url)| {
                    let result = client
                        .raw_request::<(&EthSendBundle,), EthBundleHash>(
                            "eth_sendBundle".into(),
                            (&bundle,)
                        )
                        .await
                        .inspect_err(|e| {
                            tracing::warn!(url=%url.as_str(), err=%e, "failed to submit to mev-boost");
                        });
                    (url, result)
                })
                .buffer_unordered(DEFAULT_SUBMISSION_CONCURRENCY)
                .collect::<Vec<_>>()
                .await;

            let latency_ms = start.elapsed().as_millis() as u64;

            // Find the first successful endpoint
            let successful_endpoint = results
                .into_iter()
                .find(|(_, result)| result.is_ok())
                .map(|(url, _)| url.to_string());

            match successful_endpoint {
                Some(endpoint) => Ok(Some(SubmissionResult {
                    tx_hash: Some(hash),
                    submitter_type: "mev_boost".to_string(),
                    endpoint,
                    latency_ms
                })),
                None => Ok(None)
            }
        })
    }
}

/// A [`Signer`] wrapper to sign bundles.
#[derive(Clone)]
pub struct BundleSigner {
    /// The header name on which set the signature.
    pub header: String,
    /// The signer used to sign the bundle.
    pub signer: Arc<dyn Signer + Send + Sync>
}

impl BundleSigner {
    /// Creates a new [`BundleSigner`]
    pub fn new<S>(header: String, signer: S) -> Self
    where
        S: Signer + Send + Sync + 'static
    {
        Self { header, signer: Arc::new(signer) }
    }

    /// Creates a [`BundleSigner`] set up to add the Flashbots header.
    pub fn flashbots<S>(signer: S) -> Self
    where
        S: Signer + Send + Sync + 'static
    {
        Self { header: "X-Flashbots-Signature".to_string(), signer: Arc::new(signer) }
    }

    /// Returns the signer address.
    pub fn address(&self) -> Address {
        self.signer.address()
    }
}

impl Debug for BundleSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BundleSigner")
            .field("header", &self.header)
            .field("signer_address", &self.signer.address())
            .finish()
    }
}

#[derive(Clone)]
struct MevHttp {
    endpoint: Url,
    signer:   BundleSigner,
    http:     reqwest::Client
}

impl MevHttp {
    pub fn new_flashbots<S>(endpoint: Url, signer: S) -> Self
    where
        S: Signer + Send + Sync + 'static
    {
        Self { signer: BundleSigner::flashbots(signer), endpoint, http: reqwest::Client::new() }
    }

    fn send_request(&self, req: RequestPacket) -> TransportFut<'static> {
        let this = self.clone();

        Box::pin(async move {
            let body = serde_json::to_vec(&req).map_err(TransportError::ser_err)?;

            let signature = this
                .signer
                .signer
                .sign_message(format!("{:?}", keccak256(&body)).as_bytes())
                .await
                .map_err(TransportErrorKind::custom)?;

            this.http
                .post(this.endpoint)
                .header(
                    &this.signer.header,
                    format!("{:?}:0x{}", this.signer.address(), hex::encode(signature.as_bytes()))
                )
                .body(body)
                .send()
                .await
                .map_err(TransportErrorKind::custom)?
                .json()
                .await
                .map_err(TransportErrorKind::custom)
        })
    }
}

use tower::Service;
impl Service<RequestPacket> for MevHttp {
    type Error = TransportError;
    type Future = TransportFut<'static>;
    type Response = ResponsePacket;

    #[inline]
    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    #[inline]
    fn call(&mut self, req: RequestPacket) -> Self::Future {
        self.send_request(req)
    }
}
