use alloy::primitives::B256;
use angstrom_types::contract_payloads::angstrom::{AngstromBundle, BundleGasDetails};
use futures::Future;
use tokio::sync::oneshot;

use crate::{ValidationClient, ValidationRequest};

pub trait BundleValidatorHandle: Send + Sync + Clone + Unpin + 'static {
    /// Simulates `bundle` against `parent_hash`'s post-state in an H+1
    /// environment. The parent is named by the caller so the bundle and the
    /// state it was priced against cannot drift apart; the result carries it
    /// back.
    fn fetch_gas_for_bundle(
        &self,
        bundle: AngstromBundle,
        parent_hash: B256
    ) -> impl Future<Output = eyre::Result<BundleGasDetails>> + Send;
}

impl BundleValidatorHandle for ValidationClient {
    async fn fetch_gas_for_bundle(
        &self,
        bundle: AngstromBundle,
        parent_hash: B256
    ) -> eyre::Result<BundleGasDetails> {
        let (tx, rx) = oneshot::channel();
        self.0
            .send(ValidationRequest::Bundle { sender: tx, bundle, parent_hash })?;

        rx.await?
    }
}
