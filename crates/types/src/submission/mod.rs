pub mod angstrom;
pub mod mempool;
pub mod mev_boost;
use std::{ops::Deref, pin::Pin, sync::Arc};

use alloy::{
    consensus::{EthereumTxEnvelope, TxEip4844Variant},
    eips::eip1559::{ETHEREUM_BLOCK_GAS_LIMIT_30M, Eip1559Estimation},
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
    contract_bindings::angstrom::Angstrom, contract_payloads::angstrom::AngstromBundle,
    primitive::AngstromSigner
};

pub(super) const EXTRA_GAS: u128 = (cfg!(feature = "testnet-sepolia") as u128) * (2e9 as u128);
const DEFAULT_SUBMISSION_CONCURRENCY: usize = 10;

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

    fn submit<'a>(
        &'a self,
        signer: &'a AngstromSigner,
        bundle: Option<&'a AngstromBundle>,
        tx_features: &'a TxFeatureInfo
    ) -> Pin<Box<dyn Future<Output = eyre::Result<Option<TxHash>>> + Send + 'a>>;

    fn build_tx(
        &self,
        signer: &AngstromSigner,
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
            .with_gas_limit(ETHEREUM_BLOCK_GAS_LIMIT_30M)
            .with_max_fee_per_gas(tx_features.fees.max_fee_per_gas + EXTRA_GAS)
            .with_max_priority_fee_per_gas(tx_features.fees.max_priority_fee_per_gas + EXTRA_GAS)
    }

    fn build_and_sign_tx<'a>(
        &'a self,
        signer: &'a AngstromSigner,
        bundle: &'a AngstromBundle,
        tx_features: &'a TxFeatureInfo
    ) -> Pin<Box<dyn Future<Output = EthereumTxEnvelope<TxEip4844Variant>> + Send + Sync + 'a>>
    {
        Box::pin(async move {
            self.build_tx(signer, bundle, tx_features)
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
    pub submitters:    Vec<Box<dyn ChainSubmitter>>
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
    pub fn new(
        node_provider: Arc<P>,
        mempool: &[Url],
        angstrom: &[Url],
        mev_boost: &[Url],
        angstom_address: Address
    ) -> Self {
        let mempool =
            Box::new(MempoolSubmitter::new(mempool, angstom_address)) as Box<dyn ChainSubmitter>;
        let angstrom =
            Box::new(AngstromSubmitter::new(angstrom, angstom_address)) as Box<dyn ChainSubmitter>;
        let mev_boost =
            Box::new(MevBoostSubmitter::new(mev_boost, angstom_address)) as Box<dyn ChainSubmitter>;

        Self { node_provider, submitters: vec![mempool, angstrom, mev_boost] }
    }

    pub async fn submit_tx(
        &self,
        signer: AngstromSigner,
        bundle: Option<AngstromBundle>,
        target_block: u64
    ) -> eyre::Result<Option<TxHash>> {
        let from = signer.address();
        let nonce = self.node_provider.get_transaction_count(from).await?;

        let fees = self.node_provider.estimate_eip1559_fees().await?;
        let chain_id = self.node_provider.get_chain_id().await?;

        let tx_features = TxFeatureInfo { nonce, fees, chain_id, target_block };

        let mut futs = Vec::new();
        for submitter in &self.submitters {
            futs.push(submitter.submit(&signer, bundle.as_ref(), &tx_features));
        }
        let mut buffered_futs = futures::stream::iter(futs).buffer_unordered(10);

        let mut tx_hash = None;
        while let Some(res) = buffered_futs.next().await {
            tx_hash = res?;
        }

        Ok(tx_hash)
    }
}
