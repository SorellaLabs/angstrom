use angstrom_types::submission::{
    AngstromBundle, AngstromMetaSigner, AngstromSigner, ChainSubmitter, ChainSubmitterHolder,
    ChainSubmitterWrapper, TxFeatureInfo,
};
use alloy_primitives::Address;
use alloy_primitives::TxHash;
use eyre::Result;
use std::future::Future;
use std::pin::Pin;

/// Minimal OP Stack submitter stub. Implements `ChainSubmitter` and returns
/// `Ok(None)` for now. Real submission logic will be added next.
#[derive(Clone, Debug)]
pub struct OpStackSequencerSubmitter {
    angstrom_address: Address,
    /// Optional: L2 HTTP RPC endpoint; when provided, the submitter can self-derive
    /// nonce/fees/chain_id and dispatch transactions directly.
    l2_http_rpc:      Option<String>,
}

impl OpStackSequencerSubmitter {
    /// Create a new OP Stack submitter for a given Angstrom address.
    pub fn new(angstrom_address: Address) -> Self {
        Self { angstrom_address, l2_http_rpc: None }
    }

    /// Helper to wrap this submitter with a signer into a `ChainSubmitterWrapper` that can be
    /// plugged into `SubmissionHandler::new_with_submitters`.
    pub fn into_wrapper<S: AngstromMetaSigner + 'static>(
        self,
        signer: AngstromSigner<S>,
    ) -> Box<dyn ChainSubmitterWrapper> {
        Box::new(ChainSubmitterHolder::new(self, signer))
    }

    /// Configure the L2 HTTP RPC endpoint to enable direct submission.
    pub fn with_l2_http_rpc(mut self, http_url: impl Into<String>) -> Self {
        self.l2_http_rpc = Some(http_url.into());
        self
    }
}

impl ChainSubmitter for OpStackSequencerSubmitter {
    fn angstrom_address(&self) -> Address {
        self.angstrom_address
    }

    fn submit<'a, S: AngstromMetaSigner>(
        &'a self,
        _signer: &'a AngstromSigner<S>,
        _bundle: Option<&'a AngstromBundle>,
        _tx_features: &'a TxFeatureInfo,
    ) -> Pin<Box<dyn Future<Output = Result<Option<TxHash>>> + Send + 'a>> {
        Box::pin(async move { Ok(None) })
    }
}
