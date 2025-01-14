use std::pin::Pin;

use alloy::{
    eips::eip2718::Encodable2718,
    network::TransactionBuilder,
    primitives::TxHash,
    providers::{ext::AnvilApi, Provider}
};
use alloy_rpc_types::TransactionRequest;
use alloy_sol_types::SolCall;
use angstrom_types::{
    contract_bindings::angstrom::Angstrom, contract_payloads::angstrom::AngstromBundle,
    mev_boost::SubmitTx, primitive::AngstromSigner
};
use futures::{Future, FutureExt, StreamExt};
use pade::PadeDecode;

use crate::contracts::anvil::WalletProviderRpc;

pub struct AnvilSubmissionProvider {
    pub provider: WalletProviderRpc
}

impl SubmitTx for AnvilSubmissionProvider {
    fn submit_transaction<'a>(
        &'a self,
        signer: &'a AngstromSigner,
        tx: TransactionRequest
    ) -> Pin<Box<dyn Future<Output = (TxHash, bool)> + Send + 'a>> {
        async move {
            tracing::debug!(?tx);
            // decoded encoded payload, then apply all mock approvals + balances for the
            // given token

            let data_vec = tx.input.input.clone().unwrap().to_vec();
            let slice = data_vec.as_slice();
            // problem is we have abi enocded as bytes so we need to unabi incode
            let bytes = Angstrom::executeCall::abi_decode(slice, true)
                .unwrap()
                .encoded;
            let vecd = bytes.to_vec();
            let mut slice = vecd.as_slice();

            let bundle = AngstromBundle::pade_decode(&mut slice, None).unwrap();
            let block = self.provider.get_block_number().await.unwrap() + 1;
            let order_overrides = bundle.fetch_needed_overrides(block);
            let angstrom_address = *tx.to.as_ref().unwrap().to().unwrap();

            let _ =
                futures::stream::iter(order_overrides.into_slots_with_overrides(angstrom_address))
                    .then(|(token, slot, value)| async move {
                        self.provider
                            .anvil_set_storage_at(token, slot.into(), value.into())
                            .await
                            .expect("failed to use anvil_set_storage_at");
                    })
                    .collect::<Vec<_>>()
                    .await;

            let tx = tx.build(&signer).await.unwrap();

            let hash = *tx.tx_hash();
            let encoded = tx.encoded_2718();

            let submitted = self.provider.send_raw_transaction(&encoded).await.is_ok();
            (hash, submitted)
        }
        .boxed()
    }
}
