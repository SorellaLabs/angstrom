use std::fmt::Debug;

use alloy::{
    eips::Encodable2718,
    network::TransactionBuilder,
    primitives::Bytes,
    providers::{Provider, RootProvider},
    rpc::client::ClientBuilder
};
use alloy_primitives::Address;
use futures::stream::{StreamExt, iter};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    AngstromBundle, AngstromSigner, ChainSubmitter, DEFAULT_SUBMISSION_CONCURRENCY,
    EXTRA_GAS_LIMIT, SubmissionResult, TxFeatureInfo, Url
};
use crate::{primitive::AngstromMetaSigner, sol_bindings::rpc_orders::AttestAngstromBlockEmpty};

pub struct AngstromSubmitter {
    clients:          Vec<(RootProvider, Url)>,
    angstrom_address: Address
}

impl AngstromSubmitter {
    pub fn new(urls: &[Url], angstrom_address: Address) -> Self {
        let clients = urls
            .iter()
            .map(|url| (RootProvider::new(ClientBuilder::default().http(url.clone())), url.clone()))
            .collect_vec();

        Self { clients, angstrom_address }
    }
}

impl ChainSubmitter for AngstromSubmitter {
    fn angstrom_address(&self) -> Address {
        self.angstrom_address
    }

    fn submitter_type(&self) -> &'static str {
        "angstrom"
    }

    fn submit<'a, S: AngstromMetaSigner>(
        &'a self,
        signer: &'a AngstromSigner<S>,
        bundle: Option<&'a AngstromBundle>,
        tx_features: &'a TxFeatureInfo
    ) -> std::pin::Pin<Box<dyn Future<Output = eyre::Result<Vec<SubmissionResult>>> + Send + 'a>>
    {
        Box::pin(async move {
            let mut tx_hash = None;
            let payload = if let Some(bundle) = bundle {
                let mut tx = self.build_tx(signer, bundle, tx_features);
                let gas_used = (tx_features.bundle_gas_used)(tx.clone()).await + EXTRA_GAS_LIMIT;
                tx = tx.with_gas_limit(gas_used);
                // Angstrom integrators have max priority gas set to 0.
                tx.set_max_priority_fee_per_gas(0);

                let gas = tx.max_priority_fee_per_gas.unwrap();
                // TODO: manipulate gas before signing based of off defined rebate spec.
                // This is pending with talks with titan so leaving it for now

                let signed_tx = tx.build(signer).await.unwrap();
                tx_hash = Some(*signed_tx.hash());
                let tx_payload = Bytes::from(signed_tx.encoded_2718());

                AngstromIntegrationSubmission {
                    tx: tx_payload,
                    unlock_data: Bytes::new(),
                    max_priority_fee_per_gas: gas
                }
            } else {
                let unlock_data =
                    AttestAngstromBlockEmpty::sign_and_encode(tx_features.target_block, signer);
                let unlock_sig = AttestAngstromBlockEmpty::sign(tx_features.target_block, signer);

                let signed_tx = self
                    .build_and_sign_unlock(signer, unlock_sig, tx_features)
                    .await;
                let tx_payload = Bytes::from(signed_tx.encoded_2718());

                AngstromIntegrationSubmission { tx: tx_payload, unlock_data, ..Default::default() }
            };

            // Submit to all endpoints and collect per-endpoint timing
            let results: Vec<_> = iter(self.clients.clone())
                .map(async |(client, url)| {
                    let endpoint_start = std::time::Instant::now();
                    let result = client
                        .raw_request::<(&AngstromIntegrationSubmission,), Value>(
                            "angstrom_submitBundle".into(),
                            (&payload,)
                        )
                        .await
                        .map(|v| AngstromSubmissionResponse::from_value(v))
                        .inspect_err(|e| {
                            tracing::info!(url=%url.as_str(), err=%e, "failed to send angstrom integration message to url");
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
                    let success = match result {
                        Ok(resp) => {
                            match &resp {
                                AngstromSubmissionResponse::Success { message, bundle_hash } => {
                                    tracing::info!(url=%url.as_str(), ?message, ?bundle_hash, "angstrom bundle submitted");
                                }
                                AngstromSubmissionResponse::Error { message } => {
                                    tracing::warn!(url=%url.as_str(), %message, "angstrom submission error");
                                }
                                AngstromSubmissionResponse::Unknown { raw } => {
                                    tracing::warn!(url=%url.as_str(), %raw, "angstrom submission unknown response format");
                                }
                            }
                            resp.is_success()
                        }
                        Err(_) => false
                    };

                    SubmissionResult {
                        tx_hash: if success { tx_hash } else { None },
                        submitter_type: "angstrom".to_string(),
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

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct AngstromIntegrationSubmission {
    pub tx: Bytes,
    pub unlock_data: Bytes,
    pub max_priority_fee_per_gas: u128
}

/// Response from angstrom integration endpoints.
/// Each endpoint returns a different format - we handle the known formats
/// explicitly.
#[derive(Debug, Clone)]
pub enum AngstromSubmissionResponse {
    Success { message: Option<String>, bundle_hash: Option<String> },
    Error { message: String },
    Unknown { raw: String }
}

impl AngstromSubmissionResponse {
    pub fn from_value(value: Value) -> Self {
        let message_field = || {
            value
                .get("message")
                .and_then(Value::as_str)
                .map(String::from)
        };

        // Format 1: {"success": bool, "message": "..."}
        match value.get("success").and_then(Value::as_bool) {
            Some(true) => return Self::Success { message: message_field(), bundle_hash: None },
            Some(false) => {
                return Self::Error {
                    message: message_field().unwrap_or_else(|| "Request failed".to_string())
                }
            }
            None => {}
        }

        // Format 2: {"jsonrpc": "2.0", "result": "...", "error": ...}
        match value.get("jsonrpc") {
            Some(_) => {
                return match value.get("error").filter(|e| !e.is_null()) {
                    Some(err) => Self::Error {
                        message: err
                            .get("message")
                            .and_then(Value::as_str)
                            .or_else(|| err.as_str())
                            .unwrap_or("Unknown error")
                            .to_string()
                    },
                    None => Self::Success {
                        message:     value
                            .get("result")
                            .and_then(Value::as_str)
                            .map(String::from),
                        bundle_hash: None
                    }
                }
            }
            None => {}
        }

        // Format 3: integer status code (e.g., 200)
        match value.as_u64() {
            Some(n @ 200..300) => {
                return Self::Success {
                    message:     Some(format!("Status: {n}")),
                    bundle_hash: None
                }
            }
            Some(n) => return Self::Error { message: format!("Error status: {n}") },
            None => {}
        }

        // Format 4: {"bundleHash": "0x..."}
        match value.get("bundleHash").and_then(Value::as_str) {
            Some(hash) => Self::Success { message: None, bundle_hash: Some(hash.to_string()) },
            None => Self::Unknown { raw: value.to_string() }
        }
    }

    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }
}
