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
}

impl OpStackSequencerSubmitter {
    /// Create a new OP Stack submitter for a given Angstrom address.
    pub fn new(angstrom_address: Address) -> Self {
        Self { angstrom_address }
    }

    /// Helper to wrap this submitter with a signer into a `ChainSubmitterWrapper` that can be
    /// plugged into `SubmissionHandler::new_with_submitters`.
    pub fn into_wrapper<S: AngstromMetaSigner + 'static>(
        self,
        signer: AngstromSigner<S>,
    ) -> Box<dyn ChainSubmitterWrapper> {
        Box::new(ChainSubmitterHolder::new(self, signer))
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
