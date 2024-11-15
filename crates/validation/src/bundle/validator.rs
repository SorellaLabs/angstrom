use angstrom_types::contract_payloads::angstrom::AngstromBundle;
use futures::Future;
use tokio::sync::oneshot;

use super::BundleResponse;
use crate::{ValidationClient, ValidationRequest};

pub trait BundleValidatorHandle: Send + Sync + Clone + Unpin + 'static {
    fn fetch_gas_for_bundle(
        &self,
        bundle: AngstromBundle
    ) -> impl Future<Output = eyre::Result<BundleResponse>> + Send;
}

impl BundleValidatorHandle for ValidationClient {
    async fn fetch_gas_for_bundle(&self, bundle: AngstromBundle) -> eyre::Result<BundleResponse> {
        let (tx, rx) = oneshot::channel();
        self.0
            .send(ValidationRequest::Bundle { sender: tx, bundle })?;

        rx.await?
    }
}
