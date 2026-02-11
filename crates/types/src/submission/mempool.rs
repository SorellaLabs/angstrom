use alloy::{
    eips::Encodable2718,
    providers::{Provider, ProviderBuilder, RootProvider}
};
use alloy_primitives::Address;
use futures::stream::{StreamExt, iter};

use super::{
    AngstromBundle, AngstromSigner, ChainSubmitter, DEFAULT_SUBMISSION_CONCURRENCY,
    SubmissionResult, TxFeatureInfo, Url
};
use crate::primitive::AngstromMetaSigner;

/// handles submitting transaction to
pub struct MempoolSubmitter {
    clients:          Vec<(RootProvider, Url)>,
    angstrom_address: Address
}

impl MempoolSubmitter {
    pub fn new(clients: &[Url], angstrom_address: Address) -> Self {
        let clients = clients
            .iter()
            .map(|url| {
                (ProviderBuilder::<_, _, _>::default().connect_http(url.clone()), url.clone())
            })
            .collect::<Vec<_>>();
        Self { clients, angstrom_address }
    }
}

impl ChainSubmitter for MempoolSubmitter {
    fn angstrom_address(&self) -> Address {
        self.angstrom_address
    }

    fn submitter_type(&self) -> &'static str {
        "mempool"
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

            let encoded_tx = tx.encoded_2718();
            let tx_hash = *tx.tx_hash();

            // Clone here is fine as its in a Arc
            let results: Vec<_> = iter(self.clients.clone())
                .map(async |(client, url)| {
                    let result = client
                        .send_raw_transaction(&encoded_tx)
                        .await
                        .inspect_err(|e| {
                            tracing::info!(url=%url.as_str(), err=%e, "failed to send mempool tx");
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
                    tx_hash: Some(tx_hash),
                    submitter_type: "mempool".to_string(),
                    endpoint,
                    latency_ms
                })),
                None => Ok(None)
            }
        })
    }
}
