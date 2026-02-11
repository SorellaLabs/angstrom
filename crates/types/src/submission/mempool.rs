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
    ) -> std::pin::Pin<Box<dyn Future<Output = eyre::Result<Vec<SubmissionResult>>> + Send + 'a>>
    {
        Box::pin(async move {
            let bundle = match bundle {
                Some(b) => b,
                None => return Ok(Vec::new()) // No bundle means no mempool submission
            };

            let tx = self
                .build_and_sign_tx_with_gas(signer, bundle, tx_features)
                .await;

            let encoded_tx = tx.encoded_2718();
            let tx_hash = *tx.tx_hash();

            // Submit to all endpoints and collect per-endpoint timing
            let results: Vec<_> = iter(self.clients.clone())
                .map(async |(client, url)| {
                    let endpoint_start = std::time::Instant::now();
                    let result = client
                        .send_raw_transaction(&encoded_tx)
                        .await
                        .inspect_err(|e| {
                            tracing::info!(url=%url.as_str(), err=%e, "failed to send mempool tx");
                        });
                    let endpoint_latency = endpoint_start.elapsed().as_millis() as u64;
                    (url, result, endpoint_latency)
                })
                .buffer_unordered(DEFAULT_SUBMISSION_CONCURRENCY)
                .collect::<Vec<_>>()
                .await;

            // Convert all results to SubmissionResult
            let submission_results: Vec<SubmissionResult> = results
                .into_iter()
                .map(|(url, result, endpoint_latency)| {
                    let success = result.is_ok();
                    SubmissionResult {
                        tx_hash: if success { Some(tx_hash) } else { None },
                        submitter_type: "mempool".to_string(),
                        endpoint: url.to_string(),
                        success,
                        latency_ms: endpoint_latency
                    }
                })
                .collect();

            Ok(submission_results)
        })
    }
}
