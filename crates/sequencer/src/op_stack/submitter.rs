use angstrom_types::submission::{
    AngstromBundle, AngstromMetaSigner, AngstromSigner, ChainSubmitter, ChainSubmitterHolder,
    ChainSubmitterWrapper, TxFeatureInfo, EXTRA_GAS_LIMIT,
};
use alloy_primitives::Address;
use alloy_primitives::TxHash;
use eyre::Result;
use std::future::Future;
use std::pin::Pin;
#[cfg(feature = "op-stack")]
use alloy_provider::{Provider, ProviderBuilder, RootProvider};

/// Minimal OP Stack submitter stub. Implements `ChainSubmitter` and returns
/// `Ok(None)` for now. Real submission logic will be added next.
#[derive(Clone, Debug)]
pub struct OpStackSequencerSubmitter {
    angstrom_address: Address,
    /// Optional: L2 HTTP RPC endpoint; when provided, the submitter can self-derive
    /// nonce/fees/chain_id and dispatch transactions directly.
    l2_http_rpc:      Option<String>,
}

impl OpStackSequencerSubmitter {
    /// Create a new OP Stack submitter for a given Angstrom address.
    pub fn new(angstrom_address: Address) -> Self {
        Self { angstrom_address, l2_http_rpc: None }
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
                let client: RootProvider = ProviderBuilder::default()
                    .with_recommended_fillers()
                    .on_http(http.parse().map_err(|e| eyre::eyre!("invalid L2 HTTP: {e}"))?);

                // We still require a bundle to submit; otherwise no-op.
                let bundle = match _bundle { Some(b) => b, None => return Ok(None) };

                // Derive fees/nonce/chain_id from L2 provider regardless of passed tx_features.
                let from = _signer.address();
                let latest_block = client.get_block_number().await?;
                let nonce = client.get_transaction_count(from).number(latest_block).await?;
                let fees = client.estimate_eip1559_fees().await?;
                let chain_id = client.get_chain_id().await?;
                let tx_features = TxFeatureInfo { nonce, fees, chain_id, target_block: latest_block };

                // Gas estimate and sign, mirroring mempool submitter
                let tx = self
                    .build_and_sign_tx_with_gas(_signer, bundle, &tx_features, |tx| async {
                        let gas = client
                            .estimate_gas(tx.clone())
                            .await
                            .unwrap_or(bundle.crude_gas_estimation())
                            + EXTRA_GAS_LIMIT;
                        tx.with_gas_limit(gas)
                    })
                    .await;

                let encoded_tx = tx.encoded_2718();
                let tx_hash = *tx.tx_hash();
                client
                    .send_raw_transaction(&encoded_tx)
                    .await
                    .inspect_err(|e| tracing::warn!(err=%e, "failed to send L2 tx"))?;
                return Ok(Some(tx_hash));
            }

            #[allow(unreachable_code)]
            Ok(None)
        })
    }
}
