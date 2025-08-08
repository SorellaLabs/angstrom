//! Sequencer integration facade for L2 chains.
//!
//! This crate provides the public traits and configuration types to interact
//! with L2 sequencer endpoints for submission today, and for block streaming
//! in the future. No networking or chain-specific logic is implemented yet.

#![deny(missing_docs)]
use angstrom_types::primitive::{AngstromMetaSigner, AngstromSigner};
use angstrom_types::submission::ChainSubmitterWrapper;

/// Feature-gated modules for OP Stack chains live here in the future.
#[cfg(feature = "op-stack")]
pub mod op_stack {
    /// OP Stack submitter and future modules.
    pub mod submitter;
    /// Placeholder for OP Stack-specific configuration.
    #[derive(Clone, Debug)]
    pub struct OpStackConfig {
        /// Human-readable chain label (e.g., "unichain", "base").
        pub chain_label: String,
        /// HTTP RPC endpoint to submit transactions.
        pub http_rpc_url: String,
        /// Optional WS endpoint for future block subscriptions.
        pub ws_rpc_url: Option<String>,
        /// Chain ID for signing.
        pub chain_id: u64,
    }
}

/// A handle that can provide transaction submission to a sequencer via the
/// existing submission plumbing (`ChainSubmitter` via `ChainSubmitterWrapper`).
///
/// Implementors are expected to return an object that implements Angstrom's
/// `ChainSubmitter` (wrapped) for integration into `SubmissionHandler`.
pub trait SequencerClient {
    /// Returns a unique name for telemetry and logging.
    fn name(&self) -> &'static str;

    /// Returns whether the client is currently healthy.
    fn is_healthy(&self) -> bool;

    /// Future-facing: prepare internal streaming resources without exposing
    /// a public stream yet.
    /// Implementations may no-op until block streaming is added.
    fn warmup_streaming(&self) {}

    /// Returns a submission wrapper that integrates with Angstrom's
    /// `SubmissionHandler`. Implementations should wrap their internal
    /// submitter with the provided signer.
    fn submitter_wrapper<S: AngstromMetaSigner + 'static>(
        &self,
        signer: AngstromSigner<S>,
    ) -> Box<dyn ChainSubmitterWrapper>;
}

/// Marker type for future block stream kinds (unsafe/safe/finalized).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreamKind {
    /// OP Stack "unsafe" head (sequencer view).
    Unsafe,
    /// OP Stack "safe" head (post-derivation stabilization).
    Safe,
    /// Finalized head.
    Finalized,
}

#[cfg(feature = "op-stack")]
pub use op_stack::submitter::OpStackSequencerSubmitter;

/// A minimal concrete `SequencerClient` backed by an OP Stack submitter.
#[cfg(feature = "op-stack")]
#[derive(Debug)]
pub struct OpSequencerClient {
    name:      &'static str,
    healthy:   std::sync::atomic::AtomicBool,
    submitter: OpStackSequencerSubmitter,
}

#[cfg(feature = "op-stack")]
impl Clone for OpSequencerClient {
    fn clone(&self) -> Self {
        Self {
            name: self.name,
            healthy: std::sync::atomic::AtomicBool::new(
                self.healthy.load(std::sync::atomic::Ordering::Relaxed),
            ),
            submitter: self.submitter.clone(),
        }
    }
}

#[cfg(feature = "op-stack")]
impl OpSequencerClient {
    /// Construct a new client. Health defaults to true; callers can set based on checks.
    pub fn new(name: &'static str, angstrom_address: alloy_primitives::Address) -> Self {
        Self {
            name,
            healthy:   std::sync::atomic::AtomicBool::new(true),
            submitter: OpStackSequencerSubmitter::new(angstrom_address),
        }
    }

    /// Update health status.
    pub fn set_health(&self, is_healthy: bool) {
        self.healthy.store(is_healthy, std::sync::atomic::Ordering::Relaxed);
    }

    /// Configure the L2 HTTP RPC endpoint to enable direct submission.
    pub fn with_l2_http_rpc(mut self, http_url: impl Into<String>) -> Self {
        self.submitter = self.submitter.clone().with_l2_http_rpc(http_url);
        self
    }

    /// Configure an explicit chain ID for L2 submission.
    pub fn with_l2_chain_id(mut self, chain_id: u64) -> Self {
        self.submitter = self.submitter.clone().with_l2_chain_id(chain_id);
        self
    }
}

#[cfg(feature = "op-stack")]
impl SequencerClient for OpSequencerClient {
    fn name(&self) -> &'static str { self.name }
    fn is_healthy(&self) -> bool { self.healthy.load(std::sync::atomic::Ordering::Relaxed) }

    fn submitter_wrapper<S: AngstromMetaSigner + 'static>(
        &self,
        signer: AngstromSigner<S>,
    ) -> Box<dyn ChainSubmitterWrapper> {
        self.submitter.clone().into_wrapper(signer)
    }
}
