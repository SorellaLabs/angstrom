use std::{
    fmt::Debug,
    sync::Arc,
    task::{Context, Poll}
};

use alloy::{
    hex,
    network::TransactionBuilder,
    primitives::keccak256,
    providers::{Provider, RootProvider},
    rpc::{
        client::ClientBuilder,
        json_rpc::{RequestPacket, ResponsePacket}
    },
    signers::Signer,
    transports::{TransportError, TransportErrorKind, TransportFut}
};
use alloy_primitives::{Address, TxHash};
use futures::stream::{StreamExt, iter};
use itertools::Itertools;
use reth::rpc::types::mev::EthSendPrivateTransaction;

use super::{
    AngstromBundle, AngstromSigner, ChainSubmitter, DEFAULT_SUBMISSION_CONCURRENCY,
    EXTRA_GAS_LIMIT, TxFeatureInfo, Url
};
use crate::primitive::AngstromMetaSigner;

pub struct MevBoostSubmitter {
    clients:          Vec<RootProvider>,
    angstrom_address: Address
}

impl MevBoostSubmitter {
    pub fn new<S: AngstromMetaSigner>(urls: &[Url], signer: AngstromSigner<S>) -> Self {
        let angstrom_address = signer.address();
        let clients = urls
            .iter()
            .map(|url| {
                let transport = MevHttp::new_flashbots(url.clone(), (*signer).clone());
                let client = ClientBuilder::default().transport(transport, false);
                RootProvider::new(client)
            })
            .collect_vec();

        Self { clients, angstrom_address }
    }
}

impl ChainSubmitter for MevBoostSubmitter {
    fn angstrom_address(&self) -> Address {
        self.angstrom_address
    }

    fn submit<'a, S: AngstromMetaSigner>(
        &'a self,
        signer: &'a AngstromSigner<S>,
        bundle: Option<&'a AngstromBundle>,
        tx_features: &'a TxFeatureInfo
    ) -> std::pin::Pin<Box<dyn Future<Output = eyre::Result<Option<TxHash>>> + Send + 'a>> {
        Box::pin(async move {
            let bundle = bundle.ok_or_else(|| eyre::eyre!("no bundle was past in"))?;

            let client = self
                .clients
                .first()
                .ok_or(eyre::eyre!("no mev-boost clients found"))?
                .clone();

            let tx = self
                .build_and_sign_tx_with_gas(signer, bundle, tx_features, |tx| async move {
                    let gas = client
                        .estimate_gas(tx.clone())
                        .await
                        .unwrap_or(bundle.crude_gas_estimation())
                        + EXTRA_GAS_LIMIT;

                    tx.with_gas_limit(gas)
                })
                .await;

            let hash = *tx.tx_hash();

            let private_tx = EthSendPrivateTransaction::new(&tx)
                .max_block_number(tx_features.target_block)
                .with_preferences(
                    reth::rpc::types::mev::PrivateTransactionPreferences::default().into_fast()
                );

            // Clone here is fine as its in a Arc
            let _: Vec<_> = iter(self.clients.clone())
                .map(async |client| {
                    client
                        .raw_request::<(&EthSendPrivateTransaction,), TxHash>(
                            "eth_sendPrivateTransaction".into(),
                            (&private_tx,)
                        )
                        .await
                        .inspect_err(|e| {
                            tracing::warn!(err=%e, "failed to submit to mev-boost");
                        })
                })
                .buffer_unordered(DEFAULT_SUBMISSION_CONCURRENCY)
                .collect::<Vec<_>>()
                .await;

            Ok(Some(hash))
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
        let signer = BundleSigner::flashbots(signer);
        Self { signer, endpoint, http: reqwest::Client::new() }
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
