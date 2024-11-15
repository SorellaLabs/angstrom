use angstrom_types::contract_payloads::angstrom::{AngstromBundle, BundleGasDetails};
use futures::Future;
use tokio::sync::oneshot;

use crate::{ValidationClient, ValidationRequest};

pub trait BundleValidatorHandle: Send + Sync + Clone + Unpin + 'static {
    fn fetch_gas_for_bundle(
        &self,
        bundle: AngstromBundle
    ) -> impl Future<Output = eyre::Result<BundleGasDetails>> + Send;
}

impl BundleValidatorHandle for ValidationClient {
    async fn fetch_gas_for_bundle(&self, bundle: AngstromBundle) -> eyre::Result<BundleGasDetails> {
        let (tx, rx) = oneshot::channel();
        self.0
            .send(ValidationRequest::Bundle { sender: tx, bundle })?;

        rx.await?
    }
}
