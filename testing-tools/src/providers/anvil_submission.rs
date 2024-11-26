use std::pin::Pin;

use alloy::{
    eips::eip2718::Encodable2718,
    network::TransactionBuilder,
    primitives::TxHash,
    providers::{ext::AnvilApi, Provider}
};
use alloy_rpc_types::TransactionRequest;
use angstrom_types::{
    contract_payloads::angstrom::AngstromBundle, mev_boost::SubmitTx, primitive::AngstromSigner
};
use futures::{Future, FutureExt};
use pade::PadeDecode;

use crate::contracts::anvil::WalletProviderRpc;

pub struct AnvilSubmissionProvider {
    provider: WalletProviderRpc
}

impl SubmitTx for AnvilSubmissionProvider {
    fn submit_transaction<'a>(
        &'a self,
        signer: &'a AngstromSigner,
        tx: TransactionRequest
    ) -> Pin<Box<dyn Future<Output = (TxHash, bool)> + Send + 'a>> {
        async move {
            // decoded encoded payload, then apply all mock approvals + balances for the
            // given token

            let data_vec = tx.input.data.unwrap().to_vec();
            let mut slice = data_vec.as_slice();
            let bundle = AngstromBundle::pade_decode(&mut slice, None).unwrap();

            let tx = tx.build(&signer).await.unwrap();

            let hash = *tx.tx_hash();
            let encoded = tx.encoded_2718();

            let submitted = self.provider.send_raw_transaction(&encoded).await.is_ok();
            (hash, submitted)
        }
        .boxed()
    }
}

// {
//     "astId": 24,
//     "contract": "test/_mocks/MintableMockERC20.sol:MintableMockERC20",
//     "label": "balanceOf",
//     "offset": 0,
//     "slot": "1",
//     "type": "t_mapping(t_address,t_uint256)"
//   },
//   {
//     "astId": 30,
//     "contract": "test/_mocks/MintableMockERC20.sol:MintableMockERC20",
//     "label": "allowance",
//     "offset": 0,
//     "slot": "2",
//     "type": "t_mapping(t_address,t_mapping(t_address,t_uint256))"
//   }
