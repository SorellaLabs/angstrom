use angstrom_types::contract_payloads::angstrom::AngstromBundle;
use angstrom_types::primitive::{AngstromMetaSigner, AngstromSigner};
use angstrom_types::submission::{
    ChainSubmitter, ChainSubmitterHolder, ChainSubmitterWrapper, TxFeatureInfo,
};
use alloy_primitives::{Address, TxHash};
use url::Url;
use eyre::Result;
use std::future::Future;
use std::pin::Pin;
#[cfg(feature = "op-stack")]
use alloy::{
    eips::Encodable2718,
    network::TransactionBuilder,
    providers::{Provider, ProviderBuilder, RootProvider},
};
#[cfg(feature = "op-stack")]
use tracing::warn;
#[cfg(feature = "op-stack")]
use tokio::time::{sleep, Duration};
#[cfg(feature = "op-stack")]
use metrics::counter;

/// Minimal OP Stack submitter. Can be a no-op when not configured with L2 HTTP
/// RPC; otherwise submits to L2.
#[derive(Clone, Debug)]
pub struct OpStackSequencerSubmitter {
    angstrom_address: Address,
    /// Optional: L2 HTTP RPC endpoint; when provided, the submitter can self-derive
    /// nonce/fees/chain_id and dispatch transactions directly.
    l2_http_rpc:      Option<String>,
    /// Optional: Chain ID override for L2 submissions.
    l2_chain_id:      Option<u64>,
}

impl OpStackSequencerSubmitter {
    /// Create a new OP Stack submitter for a given Angstrom address.
    pub fn new(angstrom_address: Address) -> Self {
        Self { angstrom_address, l2_http_rpc: None, l2_chain_id: None }
    }

    /// Helper to wrap this submitter with a signer into a `ChainSubmitterWrapper` that can be
    /// plugged into `SubmissionHandler::new_with_submitters`.
    pub fn into_wrapper<S: AngstromMetaSigner + 'static>(
        self,
        signer: AngstromSigner<S>,
    ) -> Box<dyn ChainSubmitterWrapper> {
        Box::new(ChainSubmitterHolder::new(self, signer))
    }

    /// Configure the L2 HTTP RPC endpoint to enable direct submission.
    pub fn with_l2_http_rpc(mut self, http_url: impl Into<String>) -> Self {
        self.l2_http_rpc = Some(http_url.into());
        self
    }

    /// Configure an explicit chain ID for L2 submission.
    pub fn with_l2_chain_id(mut self, chain_id: u64) -> Self {
        self.l2_chain_id = Some(chain_id);
        self
    }
}

impl ChainSubmitter for OpStackSequencerSubmitter {
    fn angstrom_address(&self) -> Address {
        self.angstrom_address
    }

    fn submit<'a, S: AngstromMetaSigner>(
        &'a self,
        _signer: &'a AngstromSigner<S>,
        _bundle: Option<&'a AngstromBundle>,
        _tx_features: &'a TxFeatureInfo,
    ) -> Pin<Box<dyn Future<Output = Result<Option<TxHash>>> + Send + 'a>> {
        Box::pin(async move {
            // If not configured yet, remain a no-op to avoid behavior change.
            let Some(http) = &self.l2_http_rpc else { return Ok(None) };
            let http = http.clone();

            #[cfg(feature = "op-stack")]
            {
                counter!("op_submit.attempts", 1);
                let client: RootProvider = ProviderBuilder::<_, _, _>::default()
                    .connect_http(Url::parse(&http).map_err(|e| eyre::eyre!("invalid L2 HTTP: {e}"))?);

                // We still require a bundle to submit; otherwise no-op.
                let bundle = match _bundle { Some(b) => b, None => return Ok(None) };

                // Derive fees/nonce/chain_id from L2 provider regardless of passed tx_features.
                let from = _signer.address();
                let latest_block = client.get_block_number().await?;
                let nonce = client.get_transaction_count(from).number(latest_block).await?;
                let fees = client.estimate_eip1559_fees().await?;
                let chain_id = match self.l2_chain_id { Some(id) => id, None => client.get_chain_id().await? };
                let tx_features = TxFeatureInfo { nonce, fees, chain_id, target_block: latest_block };

                // Gas estimate and sign, mirroring mempool submitter
                let tx = self
                    .build_and_sign_tx_with_gas(_signer, bundle, &tx_features, |tx| async {
                        let gas = client
                            .estimate_gas(tx.clone())
                            .await
                            .unwrap_or(bundle.crude_gas_estimation())
                            + L2_EXTRA_GAS_LIMIT;
                        tx.with_gas_limit(gas)
                    })
                    .await;

                let encoded_tx = tx.encoded_2718();
                let tx_hash = *tx.tx_hash();
                // Basic retry with exponential backoff (3 attempts)
                let mut attempt = 0u32;
                let mut last_err: Option<eyre::Report> = None;
                while attempt < 3 {
                    match client.send_raw_transaction(&encoded_tx).await {
                        Ok(_) => { last_err = None; break; }
                        Err(e) => {
                            warn!(err=%e, attempt, "failed to send L2 tx");
                            last_err = Some(eyre::eyre!(format!("{e}")));
                            attempt += 1;
                            if attempt < 3 { sleep(Duration::from_millis(200u64 << attempt)).await; }
                        }
                    }
                }
                if let Some(e) = last_err { counter!("op_submit.failures", 1); return Err(e); }
                counter!("op_submit.success", 1);
                return Ok(Some(tx_hash));
            }

            #[allow(unreachable_code)]
            Ok(None)
        })
    }
}

const L2_EXTRA_GAS_LIMIT: u64 = 100_000;
