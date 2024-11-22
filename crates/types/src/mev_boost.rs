use std::sync::Arc;

use alloy::{
    eips::eip2718::Encodable2718, network::TransactionBuilder, primitives::TxHash,
    providers::Provider, rpc::types::TransactionRequest
};
use futures::StreamExt;

use crate::primitive::AngstromSigner;

pub struct MevBoostSender<P, P2> {
    mev_boost_providers: Vec<Arc<P>>,
    node_provider:       Arc<P2>
}

impl<P, P2> MevBoostSender<P, P2>
where
    P: Provider,
    P2: Provider
{
    /// sends to all mev_boost_providers
    pub async fn sign_and_send(
        &self,
        signer: &AngstromSigner,
        tx: TransactionRequest
    ) -> (TxHash, bool) {
        let tx = tx.build(signer).await.unwrap();
        let hash = *tx.tx_hash();
        let encoded = tx.encoded_2718();

        let submitted = futures::stream::iter(self.providers.iter())
            .map(|provider| async { provider.send_raw_transaction(&encoded).await.is_ok() })
            .buffer_unordered(self.providers.len())
            .all(|res| async move { res })
            .await;

        (hash, submitted)
    }
}
