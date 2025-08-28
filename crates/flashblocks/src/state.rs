//! State for Flashblocks.

use std::sync::Arc;

use angstrom_types::{
    flashblocks::{FlashblocksPayloadV1, PendingChain},
    primitive::ChainExt
};
use parking_lot::RwLock;
use reth::{
    chainspec::EthChainSpec,
    providers::{BlockReaderIdExt, ChainSpecProvider, StateProviderFactory}
};
use reth_optimism_chainspec::OpHardforks;
use reth_optimism_primitives::OpBlock;
use reth_primitives_traits::{AlloyBlockHeader, Header, RecoveredBlock};
use tokio::sync::broadcast;

#[derive(Debug, Clone)]
pub struct PendingStateWriter<P: Clone> {
    /// The canonical chain state provider. Read-only.
    provider: P,
    /// The current pending state.
    pending:  Arc<RwLock<PendingState>>
}

impl<P> PendingStateWriter<P>
where
    P: StateProviderFactory
        + ChainSpecProvider<ChainSpec: EthChainSpec<Header = Header> + OpHardforks>
        + BlockReaderIdExt<Header = Header>
        + Clone
        + 'static
{
    pub fn new(provider: P) -> Self {
        let (tx, _) = broadcast::channel(128);
        let pending = Arc::new(RwLock::new(PendingState { sender: tx, current: None }));
        Self { pending, provider }
    }

    /// Get a reader for the pending state.
    pub fn reader(&self) -> PendingStateReader {
        PendingStateReader { pending: self.pending.clone() }
    }

    /// Handles a new canonical block.
    pub fn on_canonical_block(&self, block: &RecoveredBlock<OpBlock>) {
        // Reset the pending chain if the block number is greater than the canonical tip
        // number.
        let should_reset = self
            .pending
            .read()
            .current
            .as_ref()
            .is_some_and(|chain| chain.tip_number() <= block.number());

        if should_reset {
            let mut pending = self.pending.write();
            // TODO: Should we notify here?
            pending.current = None;
            tracing::debug!("Received canonical block {}, reset pending chain", block.number(),);
        }
    }

    /// Handles a new flashblock. Returns the pending chain.
    pub fn on_flashblock(&self, flashblock: FlashblocksPayloadV1) {
        let is_new = self.pending.read().current.is_none();

        if is_new {
            if flashblock.base.is_none() {
                tracing::error!(
                    "Received new flashblock without base block, after reset. This means
                 we most likely missed a block, and we won't update the pending chain until the \
                     next canonical block."
                );
                return;
            }

            // NOTE: Create the pending chain before acquiring the write lock for speed™️.
            let new = PendingChain::new(flashblock);
            let mut pending = self.pending.write();
            pending.current = Some(new);
        } else {
            let mut pending = self.pending.write();
            // SAFETY: We know the pending chain is not `None` because we checked it above.
            pending
                .current
                .as_mut()
                .unwrap()
                .push_flashblock(flashblock);
        }
    }
}

/// Contains the overlay state of the pending chain, aka the applied
/// Flashblocks. Read and write.
#[derive(Debug)]
pub struct PendingState {
    sender:  broadcast::Sender<Arc<PendingChain>>,
    current: Option<PendingChain>
}

/// TODO(mempirate): this should implement a trait so it can be used as a
/// provider.
///
/// Take a look at reth_db_provider.rs
/// Read only access.
#[derive(Debug, Clone)]
pub struct PendingStateReader {
    pending: Arc<RwLock<PendingState>>
}

impl PendingStateReader {
    /// Subscribe to [`PendingChain`] updates.
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<PendingChain>> {
        self.pending.read().sender.subscribe()
    }
}
