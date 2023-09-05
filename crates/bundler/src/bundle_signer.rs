use std::{
    collections::HashMap,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll}
};

use ethers_signers::{LocalWallet, Signer};
use futures::{stream::FuturesUnordered, Future, StreamExt};
use revm_primitives::{Address, B160};
use shared::{BundleSignature, CallerInfo, SafeTx, Signature};
use sim::{errors::SimError, Simulator};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BundleError {
    #[error("Failed to simulate bundle: {0:#?}")]
    SimulationError(#[from] SimError),
    #[error("Bundle reverted")]
    BundleRevert,
    #[error("Failed to sign bundle: {0:#?}")]
    SigningError(String),
    #[error("The sign request was outside of the sign period")]
    NotDelegatedSigningTime
}

type PendingSims = Pin<Box<dyn Future<Output = Result<BundleSignature, BundleError>> + Send>>;

/// deals with verifying the bundle
pub struct BundleSigner<S: Simulator + Unpin> {
    /// sim handle
    sim:          S,
    /// edsca key. in future will bls key
    key:          LocalWallet,
    /// pending sims
    pending_sims: FuturesUnordered<PendingSims>,
    call_info:    CallerInfo
}

impl<S: Simulator + Unpin + 'static> BundleSigner<S> {
    pub fn new(sim: S, key: LocalWallet) -> Self {
        Self {
            sim,
            key,
            pending_sims: FuturesUnordered::default(),
            call_info: CallerInfo {
                address:   B160::default(),
                nonce:     69,
                overrides: HashMap::new()
            }
        }
    }

    pub fn get_key(&self) -> Address {
        self.key.address().into()
    }

    pub fn verify_bundle_for_inclusion(&self, bundle: Arc<SafeTx>) -> Result<(), BundleError> {
        let hash = bundle.tx_hash();

        let handle = self.sim.clone();
        // rip
        let cloned_bundle = (*bundle).clone();
        let call_info = self.call_info.clone();
        let key = self.key.clone();

        self.pending_sims.push(Box::pin(async move {
            let sig = key
                .sign_message(hash)
                .await
                .map_err(|e| BundleError::SigningError(e.to_string()))?;

            let sign_messaged = Signature(sig);

            // if handle
            //     .simulate_sign_request(call_info, cloned_bundle)
            //     .await
            //     .map(|res| res.is_success())?
            // {
            //     Ok(BundleSignature { sig: sign_messaged, hash })
            // } else {
            Err(BundleError::BundleRevert)
            // }
        }));

        Ok(())
    }

    pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Result<BundleSignature, BundleError>> {
        if let Poll::Ready(Some(res)) = self.pending_sims.poll_next_unpin(cx) {
            return Poll::Ready(res)
        }
        Poll::Pending
    }
}
