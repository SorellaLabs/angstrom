//! State for Flashblocks.

use reth_optimism_primitives::OpBlock;
use reth_primitives_traits::RecoveredBlock;
use tokio::sync::watch;

/// Initialize the pending state writer and reader.
pub fn initialize() -> (PendingStateWriter, PendingStateReader) {
    let (tx, rx) = watch::channel(PendingState { tip: 0 });
    (PendingStateWriter { pending: tx }, PendingStateReader { pending: rx })
}

#[derive(Debug, Clone)]
pub struct PendingStateWriter {
    /// The current pending state.
    pending: watch::Sender<PendingState>
}

impl PendingStateWriter {
    pub fn on_canonical_block(&self, block: &RecoveredBlock<OpBlock>) {
        todo!("Implement")
    }
}

/// Contains the overlay state of the pending chain, aka the applied
/// Flashblocks. Read and write.
#[derive(Debug)]
pub struct PendingState {
    tip: u64
}

/// TODO(mempirate): this should implement a trait so it can be used as a
/// provider.
///
/// Take a look at reth_db_provider.rs
/// Read only access.
#[derive(Debug)]
pub struct PendingStateReader {
    pending: watch::Receiver<PendingState>
}
