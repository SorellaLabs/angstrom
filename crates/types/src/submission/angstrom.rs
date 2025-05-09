use std::fmt::Debug;

use alloy::{
    eips::Encodable2718,
    network::TransactionBuilder,
    primitives::Bytes,
    rpc::client::{ClientBuilder, RpcClient},
    signers::SignerSync,
    sol_types::{SolCall, SolStruct}
};
use alloy_primitives::{Address, TxHash};
use futures::{
    TryStreamExt,
    stream::{StreamExt, iter}
};
use itertools::Itertools;
use pade::PadeEncode;
use serde::{Deserialize, Serialize};

use super::{
    AngstromBundle, AngstromSigner, ChainSubmitter, DEFAULT_SUBMISSION_CONCURRENCY, TxFeatureInfo,
    Url
};
use crate::{
    contract_bindings::angstrom::Angstrom::unlockWithEmptyAttestationCall,
    primitive::ANGSTROM_DOMAIN, sol_bindings::rpc_orders::AttestAngstromBlockEmpty
};

pub struct AngstromSubmitter {
    clients:          Vec<RpcClient>,
    angstrom_address: Address
}

impl AngstromSubmitter {
    pub fn new(urls: &[Url], angstrom_address: Address) -> Self {
        let clients = urls
            .iter()
            .map(|url| ClientBuilder::default().http(url.clone()))
            .collect_vec();

        Self { clients, angstrom_address }
    }
}

impl ChainSubmitter for AngstromSubmitter {
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
            let payload = if let Some(bundle) = bundle {
                let tx = self.build_tx(signer, bundle, tx_features);
                let gas = tx.max_priority_fee_per_gas.unwrap();
                // TODO: manipulate gas before signing based of off defined rebate spec.
                // This is pending with talks with titan so leaving it for now

                let signed_tx = tx.build(signer).await.unwrap();
                let tx_payload = Bytes::from(signed_tx.encoded_2718());
                AngstromIntegrationSubmission {
                    tx: tx_payload,
                    unlock_data: Bytes::new(),
                    max_priority_fee_per_gas: gas
                }
            } else {
                let attestation =
                    AttestAngstromBlockEmpty { block_number: tx_features.target_block };

                let hash = attestation.eip712_signing_hash(&ANGSTROM_DOMAIN);
                // we pade encode here as we expect v, r, s which is not the standard currently
                let sig = signer.sign_hash_sync(&hash)?.pade_encode();
                let signer = signer.address();

                let unlock_data = Bytes::from(
                    unlockWithEmptyAttestationCall::new((signer, sig.into())).abi_encode()
                );
                AngstromIntegrationSubmission {
                    tx: Bytes::new(),
                    unlock_data,
                    ..Default::default()
                }
            };

            Ok(iter(self.clients.clone())
                .map(async |client| {
                    client
                        .request::<(&AngstromIntegrationSubmission,), Option<TxHash>>(
                            "angstrom_submitBundle",
                            (&payload,)
                        )
                        .await
                })
                .buffer_unordered(DEFAULT_SUBMISSION_CONCURRENCY)
                .try_collect::<Vec<_>>()
                .await?
                .pop()
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
