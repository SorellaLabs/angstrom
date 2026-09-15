pub mod angstrom;
pub mod mempool;
pub mod mev_boost;
use std::{ops::Deref, pin::Pin, sync::Arc};

use alloy::{
    consensus::{EthereumTxEnvelope, TxEip4844Variant},
    eips::{BlockNumHash, eip1559::Eip1559Estimation},
    network::TransactionBuilder,
    primitives::{Address, U256},
    providers::Provider,
    rpc::types::{
        BlockOverrides, TransactionRequest,
        simulate::{SimBlock, SimulatePayload},
        state::{AccountOverride, StateOverride}
    },
    sol_types::SolCall
};
use alloy_primitives::TxHash;
use angstrom::AngstromSubmitter;
use eyre::{bail, eyre};
use futures::StreamExt;
use mempool::MempoolSubmitter;
use mev_boost::MevBoostSubmitter;
use pade::PadeEncode;
use reqwest::Url;
pub use tokio_util::sync::CancellationToken;

use crate::{
    contract_bindings::angstrom::Angstrom,
    contract_payloads::angstrom::AngstromBundle,
    primitive::{ANGSTROM_ADDRESS, AngstromMetaSigner, AngstromSigner, CHAIN_ID},
    submission::Angstrom::unlockWithEmptyAttestationCall
};

const DEFAULT_SUBMISSION_CONCURRENCY: usize = 10;

pub(super) const EXTRA_GAS_LIMIT: u64 = 100_000;

type BundleGasFuture = Pin<Box<dyn Future<Output = eyre::Result<u64>> + Send>>;
type BundleGasEstimator = dyn Fn(TransactionRequest) -> BundleGasFuture + Send + Sync;

/// A gas limit for `tx` from executing it on the construction parent's state
/// in the environment of the block after it, plus [`EXTRA_GAS_LIMIT`].
///
/// `eth_simulateV1` is what gives both halves: it runs the call on the state
/// of the block it is pinned to, and the block override puts it in H+1, the
/// same shape bundle validation simulates. A plain `eth_estimateGas` pinned
/// to the parent would run in the parent's own environment, where Angstrom
/// rejects a second settlement in the block its last one landed in and flash
/// orders carry the wrong block number. The override is explicit rather than
/// left to the node: reth derives the next environment for a pinned simulate,
/// anvil does not.
///
/// The sender is given a fake balance because, unlike `eth_estimateGas`,
/// `eth_simulateV1` still charges gas up front against the call's default
/// gas limit (tens of millions), which the node account need not cover.
async fn bundle_gas_at<P: Provider>(
    provider: &P,
    parent: BlockNumHash,
    tx: TransactionRequest
) -> eyre::Result<u64> {
    let Some(from) = tx.from else { bail!("bundle transaction has no sender") };
    let payload = SimulatePayload {
        block_state_calls:        vec![SimBlock {
            block_overrides: Some(BlockOverrides {
                number: Some(U256::from(parent.number + 1)),
                ..Default::default()
            }),
            state_overrides: Some(StateOverride::from_iter([(
                from,
                AccountOverride { balance: Some(U256::MAX >> 1), ..Default::default() }
            )])),
            calls:           vec![tx]
        }],
        trace_transfers:          false,
        validation:               false,
        return_full_transactions: false
    };
    let blocks = provider.simulate(&payload).hash(parent.hash).await?;
    let call = blocks
        .first()
        .and_then(|block| block.calls.first())
        .ok_or_else(|| eyre!("eth_simulateV1 returned no call result"))?;
    if !call.status {
        bail!(
            "bundle reverted simulated on parent {} ({}): {:?}",
            parent.number,
            parent.hash,
            call.error
        );
    }
    // `gas_used` is net of refunds, and a limit has to cover usage before them.
    // EIP-3529 caps refunds at a fifth of that, so a quarter over `gas_used`
    // bounds it.
    Ok(call.gas_used + call.gas_used / 4 + EXTRA_GAS_LIMIT)
}

/// Result of an individual endpoint submission attempt
#[derive(Debug, Clone)]
pub struct SubmissionResult {
    /// The transaction hash if a bundle was submitted (only set on success)
    pub tx_hash:        Option<TxHash>,
    /// Type of submitter (e.g., "mempool", "angstrom", "mev_boost")
    pub submitter_type: String,
    /// The endpoint URL
    pub endpoint:       String,
    /// Whether this endpoint succeeded
    pub success:        bool,
    /// Time taken for the submission in milliseconds
    pub latency_ms:     u64
}

pub struct TxFeatureInfo {
    pub nonce:           u64,
    pub fees:            Eip1559Estimation,
    pub chain_id:        u64,
    pub target_block:    u64,
    pub bundle_gas_used: Box<BundleGasEstimator>,
    /// Cancelled when the round this submission belongs to is reset. Checked
    /// before signing and before each endpoint send: an aborted task stops at
    /// its next yield, and a send must not be the thing it does before that.
    pub cancel:          CancellationToken
}

/// a chain submitter is a trait that deals with submitting a bundle to the
/// different configured endpoints.
pub trait ChainSubmitter: Send + Sync + Unpin + 'static {
    fn angstrom_address(&self) -> Address;

    /// Returns the submitter type name for metrics
    fn submitter_type(&self) -> &'static str;

    /// Submit to all endpoints and return results for each one
    fn submit<'a, S: AngstromMetaSigner>(
        &'a self,
        signer: &'a AngstromSigner<S>,
        bundle: Option<&'a AngstromBundle>,
        tx_features: &'a TxFeatureInfo
    ) -> Pin<Box<dyn Future<Output = eyre::Result<Vec<SubmissionResult>>> + Send + 'a>>;

    fn build_tx<S: AngstromMetaSigner>(
        &self,
        signer: &AngstromSigner<S>,
        bundle: &AngstromBundle,
        tx_features: &TxFeatureInfo
    ) -> TransactionRequest {
        let encoded = Angstrom::executeCall::new((bundle.pade_encode().into(),)).abi_encode();
        TransactionRequest::default()
            .with_from(signer.address())
            .with_kind(revm_primitives::TxKind::Call(self.angstrom_address()))
            .with_input(encoded)
            .with_chain_id(tx_features.chain_id)
            .with_nonce(tx_features.nonce)
            .with_max_fee_per_gas(tx_features.fees.max_fee_per_gas)
            .with_max_priority_fee_per_gas(tx_features.fees.max_priority_fee_per_gas)
    }

    fn build_and_sign_unlock<'a, S: AngstromMetaSigner>(
        &'a self,
        signer: &'a AngstromSigner<S>,
        sig: Vec<u8>,
        tx_features: &'a TxFeatureInfo
    ) -> Pin<Box<dyn Future<Output = eyre::Result<EthereumTxEnvelope<TxEip4844Variant>>> + Send + 'a>>
    {
        Box::pin(async move {
            let unlock_call = unlockWithEmptyAttestationCall {
                node:      signer.address(),
                signature: sig.into()
            };
            // getting invalid signature
            Ok(alloy::rpc::types::TransactionRequest::default()
                .to(*ANGSTROM_ADDRESS.get().unwrap())
                .with_from(signer.address())
                .with_input(unlock_call.abi_encode())
                .with_chain_id(*CHAIN_ID.get().unwrap())
                .with_nonce(tx_features.nonce)
                .gas_limit(100_000)
                .with_max_fee_per_gas(tx_features.fees.max_fee_per_gas)
                // We can put zero here as this is only for angstrom integrators.
                .with_max_priority_fee_per_gas(0)
                .build(&signer)
                .await?)
        })
    }

    /// Estimates, then signs. Errors rather than signing if the round was reset
    /// while the estimate was in flight.
    fn build_and_sign_tx_with_gas<'a, S: AngstromMetaSigner>(
        &'a self,
        signer: &'a AngstromSigner<S>,
        bundle: &'a AngstromBundle,
        tx_features: &'a TxFeatureInfo
    ) -> Pin<Box<dyn Future<Output = eyre::Result<EthereumTxEnvelope<TxEip4844Variant>>> + Send + 'a>>
    {
        Box::pin(async move {
            let tx = self.build_tx(signer, bundle, tx_features);
            let gas = (tx_features.bundle_gas_used)(tx.clone()).await?;
            if tx_features.cancel.is_cancelled() {
                bail!("round reset before signing");
            }

            Ok(tx.gas_limit(gas + EXTRA_GAS_LIMIT).build(signer).await?)
        })
    }
}

pub struct SubmissionHandler<P>
where
    P: Provider + 'static
{
    pub node_provider: Arc<P>,
    pub submitters:    Vec<Box<dyn ChainSubmitterWrapper>>
}

impl<P> Deref for SubmissionHandler<P>
where
    P: Provider + Unpin + 'static
{
    type Target = P;

    fn deref(&self) -> &Self::Target {
        &self.node_provider
    }
}

impl<P> SubmissionHandler<P>
where
    P: Provider + 'static + Unpin
{
    pub fn new<S: AngstromMetaSigner + 'static>(
        node_provider: Arc<P>,
        mempool: &[Url],
        angstrom: &[Url],
        mev_boost: &[Url],
        angstom_address: Address,
        signer: AngstromSigner<S>
    ) -> Self {
        let mempool = Box::new(ChainSubmitterHolder::new(
            MempoolSubmitter::new(mempool, angstom_address),
            signer.clone()
        )) as Box<dyn ChainSubmitterWrapper>;
        let angstrom = Box::new(ChainSubmitterHolder::new(
            AngstromSubmitter::new(angstrom, angstom_address),
            signer.clone()
        )) as Box<dyn ChainSubmitterWrapper>;
        let mev_boost = Box::new(ChainSubmitterHolder::new(
            MevBoostSubmitter::new(mev_boost, signer.clone(), angstom_address),
            signer
        )) as Box<dyn ChainSubmitterWrapper>;

        Self { node_provider, submitters: vec![mempool, angstrom, mev_boost] }
    }

    /// Submit to all configured endpoints and return all results.
    ///
    /// `parent` is the block the bundle was built on: the nonce is read from
    /// its state and the gas estimate executes on it, in the environment of
    /// the block after it, which is the block the submission targets. A parent
    /// the node can no longer resolve is an error here, not a quiet
    /// re-preparation on whatever the tip is now.
    pub async fn submit_tx<S: AngstromMetaSigner>(
        &self,
        signer: AngstromSigner<S>,
        bundle: Option<AngstromBundle>,
        parent: BlockNumHash,
        cancel: CancellationToken
    ) -> eyre::Result<Vec<SubmissionResult>> {
        let target_block = parent.number + 1;
        let from = signer.address();
        let nonce = self
            .node_provider
            .get_transaction_count(from)
            .hash(parent.hash)
            .await?;

        let fees = self.node_provider.estimate_eip1559_fees().await?;
        let chain_id = self.node_provider.get_chain_id().await?;
        if cancel.is_cancelled() {
            bail!("round reset during submission preparation");
        }

        let node_provider = self.node_provider.clone();
        let tx_features = TxFeatureInfo {
            nonce,
            fees,
            chain_id,
            target_block,
            bundle_gas_used: Box::new(move |tx| {
                let node_provider = node_provider.clone();
                Box::pin(async move { bundle_gas_at(&*node_provider, parent, tx).await })
            }),
            cancel
        };

        let mut futs = Vec::new();
        for submitter in &self.submitters {
            futs.push(submitter.submit(bundle.as_ref(), &tx_features));
        }
        let mut buffered_futs = futures::stream::iter(futs).buffer_unordered(10);

        let mut all_results = Vec::new();
        let mut failure = None;
        // Collect all results from all submitters. A submitter only errors before
        // it sends (estimation, signing), so with no results at all the error is
        // the outcome of this submission.
        while let Some(res) = buffered_futs.next().await {
            match res {
                Ok(results) => all_results.extend(results),
                Err(e) => {
                    tracing::error!(err=%e, "submitter failed before sending");
                    failure = Some(e);
                }
            }
        }

        match failure {
            Some(e) if all_results.is_empty() => Err(e),
            _ => Ok(all_results)
        }
    }
}

pub struct ChainSubmitterHolder<I: ChainSubmitter, S: AngstromMetaSigner>(I, AngstromSigner<S>);

impl<I: ChainSubmitter, S: AngstromMetaSigner> ChainSubmitterHolder<I, S> {
    pub fn new(i: I, s: AngstromSigner<S>) -> Self {
        Self(i, s)
    }
}

pub trait ChainSubmitterWrapper: Send + Sync + Unpin + 'static {
    fn angstrom_address(&self) -> Address;

    fn submit<'a>(
        &'a self,
        bundle: Option<&'a AngstromBundle>,
        tx_features: &'a TxFeatureInfo
    ) -> Pin<Box<dyn Future<Output = eyre::Result<Vec<SubmissionResult>>> + Send + 'a>>;
}

impl<I: ChainSubmitter, S: AngstromMetaSigner + 'static> ChainSubmitterWrapper
    for ChainSubmitterHolder<I, S>
{
    fn angstrom_address(&self) -> Address {
        self.0.angstrom_address()
    }

    fn submit<'a>(
        &'a self,
        bundle: Option<&'a AngstromBundle>,
        tx_features: &'a TxFeatureInfo
    ) -> Pin<Box<dyn Future<Output = eyre::Result<Vec<SubmissionResult>>> + Send + 'a>> {
        self.0.submit(&self.1, bundle, tx_features)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Signs;

    impl ChainSubmitter for Signs {
        fn angstrom_address(&self) -> Address {
            Address::ZERO
        }

        fn submitter_type(&self) -> &'static str {
            "signs"
        }

        fn submit<'a, S: AngstromMetaSigner>(
            &'a self,
            _: &'a AngstromSigner<S>,
            _: Option<&'a AngstromBundle>,
            _: &'a TxFeatureInfo
        ) -> Pin<Box<dyn Future<Output = eyre::Result<Vec<SubmissionResult>>> + Send + 'a>>
        {
            unimplemented!()
        }
    }

    fn features(cancel: CancellationToken) -> TxFeatureInfo {
        TxFeatureInfo {
            nonce: 0,
            fees: Eip1559Estimation { max_fee_per_gas: 1, max_priority_fee_per_gas: 1 },
            chain_id: 1,
            target_block: 2,
            bundle_gas_used: Box::new(|_| Box::pin(async { Ok(21_000) })),
            cancel
        }
    }

    /// The estimate has come back and the round was reset while it was out:
    /// the transaction is not signed, so there is nothing to send.
    #[tokio::test]
    async fn a_reset_round_is_not_signed() {
        let signer = AngstromSigner::random();
        let bundle = AngstromBundle::new(vec![], vec![], vec![], vec![], vec![]);

        let live = CancellationToken::new();
        assert!(
            Signs
                .build_and_sign_tx_with_gas(&signer, &bundle, &features(live))
                .await
                .is_ok()
        );

        let reset = CancellationToken::new();
        reset.cancel();
        assert!(
            Signs
                .build_and_sign_tx_with_gas(&signer, &bundle, &features(reset))
                .await
                .is_err()
        );
    }
}
