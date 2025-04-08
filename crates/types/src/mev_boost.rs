use std::{ops::Deref, pin::Pin, str::FromStr, sync::Arc};

use alloy::{
    eips::eip2718::Encodable2718,
    network::TransactionBuilder,
    primitives::{Address, B256, TxHash},
    providers::{Provider, ProviderBuilder},
    rpc::types::{TransactionRequest, mev::PrivateTransactionRequest},
    transports::http::reqwest::Url
};
use futures::{Future, FutureExt};

use crate::primitive::AngstromSigner;

/// Allows for us to have a look at the angstrom payload to ensure that we can
/// set balances properly for when the transaction is submitted
pub trait SubmitTx: Send + Sync {
    /// submits a regular transaction using default eth endpoint
    fn submit_transaction<'a>(
        &'a self,
        signer: &'a AngstromSigner,
        tx: TransactionRequest
    ) -> Pin<Box<dyn Future<Output = (TxHash, bool)> + Send + 'a>>;

    /// uses the flashbots endpoint of "eth_sendPrivateTransaction". if not
    /// implemented, reverts to default endpoint
    fn submit_transaction_private<'a>(
        &'a self,
        signer: &'a AngstromSigner,
        tx: TransactionRequest,
        _: u64
    ) -> Pin<Box<dyn Future<Output = (TxHash, bool)> + Send + 'a>> {
        self.submit_transaction(signer, tx)
    }
}

// Default impl
impl<P: Provider> SubmitTx for P {
    fn submit_transaction<'a>(
        &'a self,
        signer: &'a AngstromSigner,
        tx: TransactionRequest
    ) -> Pin<Box<dyn Future<Output = (TxHash, bool)> + Send + 'a>> {
        async move {
            let tx = tx.build(&signer).await.unwrap();
            let hash = *tx.tx_hash();
            let encoded = tx.encoded_2718();

            let submitted = self.send_raw_transaction(&encoded).await.is_ok();
            (hash, submitted)
        }
        .boxed()
    }

    fn submit_transaction_private<'a>(
        &'a self,
        signer: &'a AngstromSigner,
        tx: TransactionRequest,
        target_block_number: u64
    ) -> Pin<Box<dyn Future<Output = (TxHash, bool)> + Send + 'a>> {
        async move {
            let tx = tx.build(&signer).await.unwrap();
            let hash = *tx.tx_hash();
            let private_tx = PrivateTransactionRequest::new(&tx)
                .max_block_number(target_block_number)
                .with_preferences(
                    reth::rpc::types::mev::PrivateTransactionPreferences::default().into_fast()
                );
            let submitted = self
                .raw_request::<(PrivateTransactionRequest,), B256>(
                    "eth_sendPrivateTransaction".into(),
                    (private_tx,)
                )
                .await;
            tracing::info!(?submitted);
            (hash, submitted.is_ok())
        }
        .boxed()
    }
}

/// On sepolia, there is a low frequency of mev-boost. This is
/// so that hopefully we can have bundles land frequently
const SEND_NORMAL: bool = false;

pub struct MevBoostProvider<P> {
    mev_boost_providers: Vec<Arc<Box<dyn SubmitTx>>>,
    default_providers:   Vec<Arc<Box<dyn SubmitTx>>>,
    node_provider:       Arc<P>
}

impl<P> MevBoostProvider<P>
where
    P: Provider + 'static
{
    pub fn new_from_raw(
        node_provider: Arc<P>,
        mev_boost_providers: Vec<Arc<Box<dyn SubmitTx>>>
    ) -> Self {
        Self { node_provider, mev_boost_providers, default_providers: vec![] }
    }

    pub fn new_from_urls(node_provider: Arc<P>, urls: &[Url], default_urls: &[String]) -> Self {
        let mev_boost_providers = urls
            .iter()
            .map(|url| {
                Arc::new(Box::new(ProviderBuilder::<_, _, _>::default().on_http(url.clone()))
                    as Box<dyn SubmitTx>)
            })
            .collect::<Vec<_>>();
        let default = default_urls
            .iter()
            .map(|url| {
                Arc::new(Box::new(
                    ProviderBuilder::<_, _, _>::default().on_http(Url::from_str(&url).unwrap())
                ) as Box<dyn SubmitTx>)
            })
            .collect::<Vec<_>>();

        Self { mev_boost_providers, node_provider, default_providers: default }
    }

    pub async fn populate_gas_nonce_chain_id(&self, tx_from: Address, tx: &mut TransactionRequest) {
        let next_nonce = self
            .node_provider
            .get_transaction_count(tx_from)
            .await
            .unwrap();

        tx.set_nonce(next_nonce);
        tx.set_gas_limit(30_000_000);
        let fees = self.node_provider.estimate_eip1559_fees().await.unwrap();
        tx.set_max_fee_per_gas(fees.max_fee_per_gas);
        tx.set_max_priority_fee_per_gas(fees.max_priority_fee_per_gas);

        let chain_id = self.node_provider.get_chain_id().await.unwrap();
        tx.set_chain_id(chain_id);
    }

    // has as consumption here due to weird to general error
    pub async fn sign_and_send(
        &self,
        signer: AngstromSigner,
        tx: TransactionRequest,
        target_block: u64
    ) -> (TxHash, bool) {
        let mut submitted = true;
        let mut phash = None;
        for provider in self.mev_boost_providers.clone() {
            let (hash, sent) = provider
                .submit_transaction_private(&signer, tx.clone(), target_block)
                .await;
            phash = Some(hash);
            submitted &= sent;
        }

        if SEND_NORMAL {
            let (hash, sent) = self
                .node_provider
                .submit_transaction(&signer, tx.clone())
                .await;
            phash = Some(hash);
            submitted &= sent;

            for default_provider in &self.default_providers {
                let (hash, sent) = default_provider
                    .submit_transaction(&signer, tx.clone())
                    .await;
                phash = Some(hash);
                submitted &= sent;
            }
        }

        (phash.expect("no mev boost endpoint was set"), submitted)
    }
}

impl<P> Deref for MevBoostProvider<P> {
    type Target = P;

    fn deref(&self) -> &Self::Target {
        &self.node_provider
    }
}
