use std::fmt::Debug;

use alloy::{
    eips::Encodable2718,
    network::TransactionBuilder,
    primitives::Bytes,
    providers::{Provider, RootProvider},
    rpc::client::ClientBuilder
};
use alloy_primitives::{Address, TxHash};
use futures::stream::{StreamExt, iter};
use itertools::Itertools;
use serde::{Deserialize, Serialize};

use super::{
    AngstromBundle, AngstromSigner, ChainSubmitter, DEFAULT_SUBMISSION_CONCURRENCY,
    EXTRA_GAS_LIMIT, TxFeatureInfo, Url
};
use crate::{primitive::AngstromMetaSigner, sol_bindings::rpc_orders::AttestAngstromBlockEmpty};

pub struct AngstromSubmitter {
    clients:          Vec<(RootProvider, Url)>,
    angstrom_address: Address
}

impl AngstromSubmitter {
    pub fn new(urls: &[Url], angstrom_address: Address) -> Self {
        let clients = urls
            .iter()
            .map(|url| (RootProvider::new(ClientBuilder::default().http(url.clone())), url.clone()))
            .collect_vec();

        Self { clients, angstrom_address }
    }
}

impl ChainSubmitter for AngstromSubmitter {
    fn angstrom_address(&self) -> Address {
        self.angstrom_address
    }

    fn submit<'a, S: AngstromMetaSigner>(
        &'a self,
        signer: &'a AngstromSigner<S>,
        bundle: Option<&'a AngstromBundle>,
        tx_features: &'a TxFeatureInfo
    ) -> std::pin::Pin<Box<dyn Future<Output = eyre::Result<Option<TxHash>>> + Send + 'a>> {
        Box::pin(async move {
            let mut tx_hash = None;
            let payload = if let Some(bundle) = bundle {
                let mut tx = self.build_tx(signer, bundle, tx_features);
                let gas_used = (tx_features.bundle_gas_used)(tx.clone()).await + EXTRA_GAS_LIMIT;
                tx = tx.with_gas_limit(gas_used);
                // Angstrom integrators have max priority gas set to 0.
                tx.set_max_priority_fee_per_gas(0);

                let gas = tx.max_priority_fee_per_gas.unwrap();
                // TODO: manipulate gas before signing based of off defined rebate spec.
                // This is pending with talks with titan so leaving it for now

                let signed_tx = tx.build(signer).await.unwrap();
                tx_hash = Some(*signed_tx.hash());
                let tx_payload = Bytes::from(signed_tx.encoded_2718());

                AngstromIntegrationSubmission {
                    tx: tx_payload,
                    unlock_data: Bytes::new(),
                    max_priority_fee_per_gas: gas
                }
            } else {
                let unlock_data =
                    AttestAngstromBlockEmpty::sign_and_encode(tx_features.target_block, signer);
                let unlock_sig = AttestAngstromBlockEmpty::sign(tx_features.target_block, signer);

                let signed_tx = self
                    .build_and_sign_unlock(signer, unlock_sig, tx_features)
                    .await;
                let tx_payload = Bytes::from(signed_tx.encoded_2718());

                AngstromIntegrationSubmission { tx: tx_payload, unlock_data, ..Default::default() }
            };

            Ok(iter(self.clients.clone())
                .map(async |(client, url)| {
                    client
                        .raw_request::<(&AngstromIntegrationSubmission,), String>(
                            "angstrom_submitBundle".into(),
                            (&payload,)
                        )
                        .await.inspect_err(|e| {
                            tracing::info!(url=%url.as_str(), err=%e, "failed to send angstrom integration message to url");
                        })
                })
                .buffer_unordered(DEFAULT_SUBMISSION_CONCURRENCY)
                .collect::<Vec<_>>()
                .await
                .into_iter()
                .flatten()
                .next()
                .map(|_| tx_hash)
                .unwrap_or_default())
        })
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct AngstromIntegrationSubmission {
    pub tx: Bytes,
    pub unlock_data: Bytes,
    pub max_priority_fee_per_gas: u128
}
