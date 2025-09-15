use alloy::{
    eips::Encodable2718,
    providers::{Provider, ProviderBuilder, RootProvider}
};
use alloy_primitives::{Address, TxHash, keccak256};
use futures::stream::{StreamExt, iter};

use super::{
    AngstromBundle, AngstromSigner, ChainSubmitter, DEFAULT_SUBMISSION_CONCURRENCY,
    NetworkProvider, TxFeatureInfo, Url
};
use crate::primitive::AngstromMetaSigner;

/// handles submitting transaction to
pub struct MempoolSubmitter {
    clients:          Vec<(RootProvider, Url)>,
    angstrom_address: Address
}

impl MempoolSubmitter {
    pub fn new(clients: &[Url], angstrom_address: Address) -> Self {
        let clients = clients
            .iter()
            .map(|url| {
                (ProviderBuilder::<_, _, _>::default().connect_http(url.clone()), url.clone())
            })
            .collect::<Vec<_>>();
        Self { clients, angstrom_address }
    }
}

impl ChainSubmitter for MempoolSubmitter {
    fn angstrom_address(&self) -> Address {
        self.angstrom_address
    }

    fn submit<'a, S: AngstromMetaSigner, N: NetworkProvider>(
        &'a self,
        signer: &'a AngstromSigner<S>,
        bundle: Option<&'a AngstromBundle>,
        tx_features: &'a TxFeatureInfo<N>
    ) -> std::pin::Pin<Box<dyn Future<Output = eyre::Result<Option<TxHash>>> + Send + 'a>> {
        Box::pin(async move {
            let bundle = bundle.ok_or_else(|| eyre::eyre!("no bundle was past in"))?;

            let tx = self
                .build_and_sign_tx_with_gas(signer, bundle, tx_features)
                .await;

            let encoded_tx = tx.encoded_2718();
            // TODO(mempirate): MAY BE WRONG
            let tx_hash = keccak256(&encoded_tx);

            // Clone here is fine as its in a Arc
            let _: Vec<_> = iter(self.clients.clone())
                .map(async |(client, url)| {
                    client
                        .send_raw_transaction(&encoded_tx)
                        .await
                        .inspect_err(|e| {
                            tracing::info!(url=%url.as_str(), err=%e, "failed to send mempool tx");
                        })
                })
                .buffer_unordered(DEFAULT_SUBMISSION_CONCURRENCY)
                .collect::<Vec<_>>()
                .await;

            Ok(Some(tx_hash))
        })
    }
}
