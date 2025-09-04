pub mod angstrom;
pub mod mempool;
pub mod mev_boost;
use std::{marker::PhantomData, ops::Deref, pin::Pin, sync::Arc};

use alloy::{
    eips::eip1559::Eip1559Estimation,
    network::{Network, TransactionBuilder},
    primitives::Address,
    providers::Provider,
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
    provider::{EthNetworkProvider, NetworkProvider},
    submission::Angstrom::unlockWithEmptyAttestationCall
};

const DEFAULT_SUBMISSION_CONCURRENCY: usize = 10;

pub(super) const EXTRA_GAS_LIMIT: u64 = 100_000;

pub struct TxFeatureInfo<N: NetworkProvider> {
    pub nonce:           u64,
    pub fees:            Eip1559Estimation,
    pub chain_id:        u64,
    pub target_block:    u64,
    pub bundle_gas_used: Box<
        dyn Fn(
                <N::Network as Network>::TransactionRequest
            ) -> Pin<Box<dyn Future<Output = u64> + Send>>
            + Send
            + Sync
    >
}

/// a chain submitter is a trait that deals with submitting a bundle to the
/// different configured endpoints.
pub trait ChainSubmitter: Send + Sync + Unpin + 'static {
    fn angstrom_address(&self) -> Address;

    fn submit<'a, S: AngstromMetaSigner, N: NetworkProvider>(
        &'a self,
        signer: &'a AngstromSigner<S>,
        bundle: Option<&'a AngstromBundle>,
        tx_features: &'a TxFeatureInfo<N>
    ) -> Pin<Box<dyn Future<Output = eyre::Result<Option<TxHash>>> + Send + 'a>>;

    fn build_tx<S: AngstromMetaSigner, N: NetworkProvider>(
        &self,
        signer: &AngstromSigner<S>,
        bundle: &AngstromBundle,
        tx_features: &TxFeatureInfo<N>
    ) -> <N::Network as Network>::TransactionRequest {
        let encoded = Angstrom::executeCall::new((bundle.pade_encode().into(),)).abi_encode();
        <N::Network as Network>::TransactionRequest::default()
            .with_from(signer.address())
            .with_kind(revm_primitives::TxKind::Call(self.angstrom_address()))
            .with_input(encoded)
            .with_chain_id(tx_features.chain_id)
            .with_nonce(tx_features.nonce)
            .with_max_fee_per_gas(tx_features.fees.max_fee_per_gas)
            .with_max_priority_fee_per_gas(tx_features.fees.max_priority_fee_per_gas)
    }

    fn build_and_sign_unlock<'a, S: AngstromMetaSigner, N: NetworkProvider>(
        &'a self,
        signer: &'a AngstromSigner<S>,
        sig: Vec<u8>,
        tx_features: &'a TxFeatureInfo<N>
    ) -> Pin<Box<dyn Future<Output = <N::Network as Network>::TxEnvelope> + Send + 'a>> {
        Box::pin(async move {
            let unlock_call = unlockWithEmptyAttestationCall {
                node:      signer.address(),
                signature: sig.into()
            };
            // getting invalid signature
            <N::Network as Network>::TransactionRequest::default()
                .with_to(*ANGSTROM_ADDRESS.get().unwrap())
                .with_from(signer.address())
                .with_input(unlock_call.abi_encode())
                .with_chain_id(*CHAIN_ID.get().unwrap())
                .with_nonce(tx_features.nonce)
                .with_gas_limit(100_000)
                .with_max_fee_per_gas(tx_features.fees.max_fee_per_gas)
                // We can put zero here as this is only for angstrom integrators.
                .with_max_priority_fee_per_gas(0)
                .build(&signer)
                .await
                .unwrap()
        })
    }

    fn build_and_sign_tx_with_gas<'a, S: AngstromMetaSigner, N: NetworkProvider>(
        &'a self,
        signer: &'a AngstromSigner<S>,
        bundle: &'a AngstromBundle,
        tx_features: &'a TxFeatureInfo<N>
    ) -> Pin<Box<dyn Future<Output = <N::Network as Network>::TxEnvelope> + Send + 'a>> {
        Box::pin(async move {
            let tx = self.build_tx(signer, bundle, tx_features);
            let gas = (tx_features.bundle_gas_used)(tx.clone()).await;

            tx.with_gas_limit(gas + EXTRA_GAS_LIMIT)
                .build(signer)
                .await
                .unwrap()
        })
    }
}

pub struct SubmissionHandler<P, N = EthNetworkProvider>
where
    N: NetworkProvider,
    P: Provider<N::Network> + 'static
{
    pub node_provider: Arc<P>,
    pub submitters:    Vec<Box<dyn ChainSubmitterWrapper<NetworkProvider = N>>>,
    pub _phantom:      PhantomData<N>
}

impl<P, N> Deref for SubmissionHandler<P, N>
where
    N: NetworkProvider,
    P: Provider<N::Network> + Unpin + 'static
{
    type Target = P;

    fn deref(&self) -> &Self::Target {
        &self.node_provider
    }
}

impl<P, N> SubmissionHandler<P, N>
where
    N: NetworkProvider + 'static,
    P: Provider<N::Network> + 'static + Unpin
{
    pub fn new<S: AngstromMetaSigner + 'static>(
        node_provider: Arc<P>,
        mempool: &[Url],
        angstom_address: Address,
        signer: AngstromSigner<S>
    ) -> Self {
        let mempool = Box::new(ChainSubmitterHolder::new(
            MempoolSubmitter::new(mempool, angstom_address),
            signer.clone()
        )) as Box<dyn ChainSubmitterWrapper<NetworkProvider = N>>;

        Self { node_provider, submitters: vec![mempool], _phantom: PhantomData }
    }

    pub fn with_angstrom<S: AngstromMetaSigner + 'static>(
        mut self,
        angstrom: &[Url],
        angstom_address: Address,
        signer: AngstromSigner<S>
    ) -> Self {
        let angstrom = Box::new(ChainSubmitterHolder::new(
            AngstromSubmitter::new(angstrom, angstom_address),
            signer
        )) as Box<dyn ChainSubmitterWrapper<NetworkProvider = N>>;

        self.submitters.push(angstrom);
        self
    }

    pub fn with_mev_boost<S: AngstromMetaSigner + 'static>(
        mut self,
        mev_boost: &[Url],
        signer: AngstromSigner<S>
    ) -> Self {
        let mev_boost = Box::new(ChainSubmitterHolder::new(
            MevBoostSubmitter::new(mev_boost, signer.clone()),
            signer
        )) as Box<dyn ChainSubmitterWrapper<NetworkProvider = N>>;

        self.submitters.push(mev_boost);
        self
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

        let node_provider = self.node_provider.clone();
        let tx_features = TxFeatureInfo {
            nonce,
            fees,
            chain_id,
            target_block,
            bundle_gas_used: Box::new(move |tx| {
                let node_provider = node_provider.clone();
                Box::pin(
                    async move { node_provider.estimate_gas(tx).await.unwrap() + EXTRA_GAS_LIMIT }
                )
            })
        };

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

pub struct ChainSubmitterHolder<I: ChainSubmitter, S: AngstromMetaSigner, N: NetworkProvider>(
    I,
    AngstromSigner<S>,
    PhantomData<N>
);

impl<I: ChainSubmitter, S: AngstromMetaSigner, N: NetworkProvider> ChainSubmitterHolder<I, S, N> {
    pub fn new(i: I, s: AngstromSigner<S>) -> Self {
        Self(i, s, PhantomData)
    }
}

pub trait ChainSubmitterWrapper: Send + Sync + Unpin + 'static {
    type NetworkProvider: NetworkProvider;

    fn angstrom_address(&self) -> Address;

    fn submit<'a>(
        &'a self,
        bundle: Option<&'a AngstromBundle>,
        tx_features: &'a TxFeatureInfo<Self::NetworkProvider>
    ) -> Pin<Box<dyn Future<Output = eyre::Result<Option<TxHash>>> + Send + 'a>>;
}

impl<I: ChainSubmitter, S: AngstromMetaSigner + 'static, N: NetworkProvider + 'static>
    ChainSubmitterWrapper for ChainSubmitterHolder<I, S, N>
{
    type NetworkProvider = N;

    fn angstrom_address(&self) -> Address {
        self.0.angstrom_address()
    }

    fn submit<'a>(
        &'a self,
        bundle: Option<&'a AngstromBundle>,
        tx_features: &'a TxFeatureInfo<Self::NetworkProvider>
    ) -> Pin<Box<dyn Future<Output = eyre::Result<Option<TxHash>>> + Send + 'a>> {
        self.0.submit(&self.1, bundle, tx_features)
    }
}
