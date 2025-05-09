use std::pin::Pin;

use alloy::{
    eips::eip2718::Encodable2718,
    primitives::{Address, TxHash},
    providers::Provider
};
use angstrom_types::{
    contract_payloads::angstrom::AngstromBundle,
    primitive::AngstromSigner,
    submission::{ChainSubmitter, TxFeatureInfo}
};
use futures::Future;

use crate::contracts::anvil::WalletProviderRpc;

pub struct AnvilSubmissionProvider {
    pub provider:         WalletProviderRpc,
    pub angstrom_address: Address
}
impl ChainSubmitter for AnvilSubmissionProvider {
    fn angstrom_address(&self) -> alloy_primitives::Address {
        self.angstrom_address
    }

    fn submit<'a>(
        &'a self,
        signer: &'a AngstromSigner,
        bundle: Option<&'a AngstromBundle>,
        tx_features: &'a TxFeatureInfo
    ) -> Pin<Box<dyn Future<Output = eyre::Result<Option<TxHash>>> + Send + 'a>> {
        Box::pin(async move {
            let Some(bundle) = bundle else { return Ok(None) };

            #[cfg(all(feature = "testnet", not(feature = "testnet-sepolia")))]
            {
                use alloy::providers::ext::AnvilApi;
                use futures::StreamExt;

                let block = self.provider.get_block_number().await.unwrap() + 1;
                let order_overrides = bundle.fetch_needed_overrides(block);
                let angstrom_address = self.angstrom_address();

                let _ = futures::stream::iter(
                    order_overrides.into_slots_with_overrides(angstrom_address)
                )
                .then(|(token, slot, value)| async move {
                    self.provider
                        .anvil_set_storage_at(token, slot.into(), value.into())
                        .await
                        .expect("failed to use anvil_set_storage_at");
                })
                .collect::<Vec<_>>()
                .await;
            }

            let tx = self.build_and_sign_tx(signer, bundle, tx_features).await;
            let hash = *tx.tx_hash();
            let encoded = tx.encoded_2718();

            self.provider
                .send_raw_transaction(&encoded)
                .await
                .map(|_| Some(hash))
                .map_err(Into::into)
        })
    }
}
