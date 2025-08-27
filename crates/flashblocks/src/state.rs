//! State for Flashblocks.

use angstrom_types::flashblocks::{FlashblocksPayloadV1, PendingChain};
use reth::{
    chainspec::EthChainSpec,
    providers::{BlockReaderIdExt, ChainSpecProvider, StateProviderFactory}
};
use reth_optimism_chainspec::OpHardforks;
use reth_optimism_primitives::OpBlock;
use reth_primitives_traits::{Header, RecoveredBlock};
use tokio::sync::watch;

#[derive(Debug, Clone)]
pub struct PendingStateWriter<P: Clone> {
    /// The canonical chain state provider. Read-only.
    provider: P,
    /// The current pending state.
    pending:  watch::Sender<PendingState>,
    /// The receiver channel for the pending state reader. Taken by the reader.
    rx:       Option<watch::Receiver<PendingState>>
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
        let (tx, rx) = watch::channel(PendingState {});
        Self { pending: tx, rx: Some(rx), provider }
    }

    /// Get a reader for the pending state.
    pub fn reader(&mut self) -> PendingStateReader {
        let rx = self.rx.take().expect("Reader already taken");
        PendingStateReader { pending: rx }
    }

    /// Handles a new canonical block.
    pub fn on_canonical_block(&self, block: &RecoveredBlock<OpBlock>) {
        todo!("Implement")
    }

    /// Handles a new flashblock. Returns the pending chain.
    pub fn on_flashblock(&self, flashblock: FlashblocksPayloadV1) -> PendingChain {
        todo!("Implement")
    }
}

/// Contains the overlay state of the pending chain, aka the applied
/// Flashblocks. Read and write.
#[derive(Debug)]
pub struct PendingState {}

/// TODO(mempirate): this should implement a trait so it can be used as a
/// provider.
///
/// Take a look at reth_db_provider.rs
/// Read only access.
#[derive(Debug)]
pub struct PendingStateReader {
    pending: watch::Receiver<PendingState>
}
