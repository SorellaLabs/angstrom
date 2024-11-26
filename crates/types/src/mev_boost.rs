use std::{marker::PhantomData, ops::Deref, pin::Pin, sync::Arc};

use alloy::{
    eips::eip2718::Encodable2718,
    network::TransactionBuilder,
    primitives::{Address, TxHash},
    providers::{Provider, ProviderBuilder, RootProvider},
    rpc::types::TransactionRequest,
    transports::{http::reqwest::Url, Transport}
};
use futures::{Future, FutureExt};

use crate::primitive::AngstromSigner;

/// Allows for us to have a look at the angstrom payload to ensure that we can
/// set balances properly for when the transaction is submitted
pub trait SubmitTx {
    fn submit_transaction<'a>(
        &'a self,
        signer: &'a AngstromSigner,
        tx: TransactionRequest
    ) -> Pin<Box<dyn Future<Output = (TxHash, bool)> + Send + 'a>>;
}

// Default impl
impl<T> SubmitTx for RootProvider<T>
where
    T: Transport + Clone
{
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
}

pub struct MevBoostProvider<P, T> {
    mev_boost_providers: Vec<Arc<Box<dyn SubmitTx>>>,
    node_provider:       Arc<P>,
    _phantom:            PhantomData<T>
}

impl<P, T> MevBoostProvider<P, T>
where
    P: Provider<T> + 'static,
    T: Transport + Clone
{
    pub fn new_from_urls(node_provider: Arc<P>, urls: &[Url]) -> Self {
        let mev_boost_providers = urls
            .iter()
            .map(|url| {
                Arc::new(Box::new(ProviderBuilder::<_, _, _>::default().on_http(url.clone()))
                    as Box<dyn SubmitTx>)
            })
            .collect::<Vec<_>>();

        Self { mev_boost_providers, node_provider, _phantom: PhantomData }
    }

    pub async fn populate_gas_nonce_chain_id(&self, tx_from: Address, tx: &mut TransactionRequest) {
        let next_nonce = self
            .node_provider
            .get_transaction_count(tx_from)
            .await
            .unwrap();

        tx.set_nonce(next_nonce);
        tx.set_gas_limit(30_000_000);
        let fees = self
            .node_provider
            .estimate_eip1559_fees(None)
            .await
            .unwrap();
        tx.set_max_fee_per_gas(fees.max_fee_per_gas);
        tx.set_max_priority_fee_per_gas(fees.max_priority_fee_per_gas);
        tx.set_chain_id(1);
    }

    // has as consumption here due to weird to general error
    pub async fn sign_and_send(
        &self,
        signer: AngstromSigner,
        tx: TransactionRequest
    ) -> (TxHash, bool) {
        let mut submitted = true;
        let mut phash = None;
        for provider in self.mev_boost_providers.clone() {
            let (hash, sent) = provider.submit_transaction(&signer, tx.clone()).await;
            phash = Some(hash);
            submitted &= sent;
        }

        (phash.expect("no mev boost endpoint was set"), submitted)
    }
}

impl<P, T> Deref for MevBoostProvider<P, T> {
    type Target = P;

    fn deref(&self) -> &Self::Target {
        &self.node_provider
    }
}
