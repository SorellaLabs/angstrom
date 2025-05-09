use alloy::{
    eips::Encodable2718,
    providers::{Provider, ProviderBuilder, RootProvider}
};
use alloy_primitives::{Address, TxHash};
use futures::{
    TryStreamExt,
    stream::{StreamExt, iter}
};

use super::{
    AngstromBundle, AngstromSigner, ChainSubmitter, DEFAULT_SUBMISSION_CONCURRENCY, TxFeatureInfo,
    Url
};

/// handles submitting transaction to
pub struct MempoolSubmitter {
    clients:          Vec<RootProvider>,
    angstrom_address: Address
}

impl MempoolSubmitter {
    pub fn new(clients: &[Url], angstrom_address: Address) -> Self {
        let clients = clients
            .iter()
            .map(|url| ProviderBuilder::<_, _, _>::default().on_http(url.clone()))
            .collect::<Vec<_>>();
        Self { clients, angstrom_address }
    }
}

impl ChainSubmitter for MempoolSubmitter {
    fn angstrom_address(&self) -> Address {
        self.angstrom_address
    }

    fn submit<'a>(
        &'a self,
        signer: &'a AngstromSigner,
        bundle: Option<&'a AngstromBundle>,
        tx_features: &'a TxFeatureInfo
    ) -> std::pin::Pin<Box<dyn Future<Output = eyre::Result<Option<TxHash>>> + Send + 'a>> {
        Box::pin(async move {
            let Some(bundle) = bundle else { return Err(eyre::eyre!("no bundle was past in")) };

            let tx = self.build_and_sign_tx(signer, bundle, tx_features).await;
            let encoded_tx = tx.encoded_2718();
            let tx_hash = *tx.tx_hash();

            // Clone here is fine as its in a Arc
            let _: Vec<_> = iter(self.clients.clone())
                .map(async |client| client.send_raw_transaction(&encoded_tx).await)
                .buffer_unordered(DEFAULT_SUBMISSION_CONCURRENCY)
                .try_collect()
                .await?;

            Ok(Some(tx_hash))
        })
    }
}
