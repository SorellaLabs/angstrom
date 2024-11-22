use std::{ops::Deref, sync::Arc};

use alloy::{
    eips::eip2718::Encodable2718,
    network::TransactionBuilder,
    primitives::{Address, TxHash},
    providers::{Provider, ProviderBuilder},
    rpc::types::TransactionRequest,
    transports::http::{reqwest::Url, ReqwestTransport}
};
use futures::StreamExt;

use crate::primitive::AngstromSigner;

pub struct MevBoostProvider<P> {
    mev_boost_providers: Vec<Box<dyn Provider<ReqwestTransport>>>,
    node_provider:       Arc<P>
}

impl<P> MevBoostProvider<P>
where
    P: Provider
{
    pub fn new_from_urls(node_provider: Arc<P>, urls: &[Url]) -> Self {
        let mev_boost_providers = urls
            .iter()
            .map(|url| {
                Box::new(ProviderBuilder::<_, _, _>::default().on_http(url.clone()))
                    as Box<dyn Provider<ReqwestTransport>>
            })
            .collect::<Vec<_>>();

        Self { mev_boost_providers, node_provider }
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

    /// sends to all mev_boost_providers
    pub async fn sign_and_send(
        &self,
        signer: &AngstromSigner,
        tx: TransactionRequest
    ) -> (TxHash, bool) {
        let tx = tx.build(signer).await.unwrap();
        let hash = *tx.tx_hash();
        let encoded = tx.encoded_2718();

        let submitted = futures::stream::iter(self.mev_boost_providers.iter())
            .map(|provider| async { provider.send_raw_transaction(&encoded).await.is_ok() })
            .buffer_unordered(self.mev_boost_providers.len())
            .all(|res| async move { res })
            .await;

        (hash, submitted)
    }
}

impl<P> Deref for MevBoostProvider<P> {
    type Target = P;

    fn deref(&self) -> &Self::Target {
        &self.node_provider
    }
}
