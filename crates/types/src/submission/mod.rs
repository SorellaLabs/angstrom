pub mod angstrom;
pub mod mempool;
pub mod mev_boost;
use std::{ops::Deref, pin::Pin, sync::Arc};

use alloy::{
    consensus::{EthereumTxEnvelope, TxEip4844Variant},
    eips::eip1559::Eip1559Estimation,
    network::TransactionBuilder,
    primitives::Address,
    providers::Provider,
    rpc::types::TransactionRequest,
    sol_types::SolCall
};
use alloy_primitives::TxHash;
use angstrom::AngstromSubmitter;
use futures::StreamExt;
use mempool::MempoolSubmitter;
use mev_boost::MevBoostSubmitter;
use pade::PadeEncode;
use reqwest::Url;

use crate::{
    contract_bindings::angstrom::Angstrom,
    contract_payloads::angstrom::AngstromBundle,
    primitive::{ANGSTROM_ADDRESS, AngstromMetaSigner, AngstromSigner, CHAIN_ID},
    submission::Angstrom::unlockWithEmptyAttestationCall
};

const DEFAULT_SUBMISSION_CONCURRENCY: usize = 10;

pub(super) const EXTRA_GAS_LIMIT: u64 = 100_000;

pub struct TxFeatureInfo {
    pub nonce:        u64,
    pub fees:         Eip1559Estimation,
    pub chain_id:     u64,
    pub target_block: u64
}

/// a chain submitter is a trait that deals with submitting a bundle to the
/// different configured endpoints.
pub trait ChainSubmitter: Send + Sync + Unpin + 'static {
    fn angstrom_address(&self) -> Address;

    fn submit<'a, S: AngstromMetaSigner>(
        &'a self,
        signer: &'a AngstromSigner<S>,
        bundle: Option<&'a AngstromBundle>,
        tx_features: &'a TxFeatureInfo
    ) -> Pin<Box<dyn Future<Output = eyre::Result<Option<TxHash>>> + Send + 'a>>;

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
    ) -> Pin<Box<dyn Future<Output = EthereumTxEnvelope<TxEip4844Variant>> + Send + 'a>> {
        Box::pin(async move {
            let unlock_call = unlockWithEmptyAttestationCall {
                node:      signer.address(),
                signature: sig.into()
            };
            // getting invalid signature
            alloy::rpc::types::TransactionRequest::default()
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
                .await
                .unwrap()
        })
    }

    fn build_and_sign_tx_with_gas<'a, S: AngstromMetaSigner, F>(
        &'a self,
        signer: &'a AngstromSigner<S>,
        bundle: &'a AngstromBundle,
        tx_features: &'a TxFeatureInfo,
        gas: impl FnOnce(TransactionRequest) -> F + Send + Sync + 'a
    ) -> Pin<Box<dyn Future<Output = EthereumTxEnvelope<TxEip4844Variant>> + Send + 'a>>
    where
        F: Future<Output = TransactionRequest> + Send + 'a
    {
        Box::pin(async move {
            gas(self.build_tx(signer, bundle, tx_features))
                .await
                .build(signer)
                .await
                .unwrap()
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
            MevBoostSubmitter::new(mev_boost, signer.clone()),
            signer
        )) as Box<dyn ChainSubmitterWrapper>;

        Self { node_provider, submitters: vec![mempool, angstrom, mev_boost] }
    }

    pub async fn submit_tx<S: AngstromMetaSigner>(
        &self,
        signer: AngstromSigner<S>,
        bundle: Option<AngstromBundle>,
        target_block: u64
    ) -> eyre::Result<Option<TxHash>> {
        let from = signer.address();
        let nonce = self
            .node_provider
            .get_transaction_count(from)
            .number(target_block - 1)
            .await?;

        let fees = self.node_provider.estimate_eip1559_fees().await?;
        let chain_id = self.node_provider.get_chain_id().await?;

        let tx_features = TxFeatureInfo { nonce, fees, chain_id, target_block };

        let mut futs = Vec::new();
        for submitter in &self.submitters {
            futs.push(submitter.submit(bundle.as_ref(), &tx_features));
        }
        let mut buffered_futs = futures::stream::iter(futs).buffer_unordered(10);

        let mut tx_hash = None;
        // We log out errors at the lower level so no need to expand them here.
        while let Some(res) = buffered_futs.next().await {
            if let Ok(Some(res)) = res {
                tx_hash = Some(res);
            }
        }

        Ok(tx_hash)
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
    ) -> Pin<Box<dyn Future<Output = eyre::Result<Option<TxHash>>> + Send + 'a>>;
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
    ) -> Pin<Box<dyn Future<Output = eyre::Result<Option<TxHash>>> + Send + 'a>> {
        self.0.submit(&self.1, bundle, tx_features)
    }
}
