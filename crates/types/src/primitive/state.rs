use std::sync::Arc;

use reth_primitives::{EthPrimitives, NodePrimitives};
use reth_provider::Chain;

use crate::flashblocks::PendingChain;

/// A canonical chain state notification. This is extended from
/// [CanonStateNotification](reth_provider::CanonStateNotification) to include
/// Flashblocks support.
#[derive(Clone, Debug)]
pub enum StateNotification<N: NodePrimitives = EthPrimitives> {
    /// The canonical chain was extended.
    Commit {
        /// The newly added chain segment.
        new: Arc<Chain<N>>
    },
    /// The flashblock was committed.
    FlashblockCommit {
        /// The newly committed flashblock.
        new: Arc<PendingChain>
    },
    /// A chain segment was reverted or reorged.
    ///
    /// - In the case of a reorg, the reverted blocks are present in `old`, and
    ///   the new blocks are present in `new`.
    /// - In the case of a revert, the reverted blocks are present in `old`, and
    ///   `new` is an empty chain segment.
    Reorg {
        /// The chain segment that was reverted.
        old: Arc<Chain<N>>,
        /// The chain segment that was added on top of the canonical chain,
        /// minus the reverted blocks.
        ///
        /// In the case of a revert, not a reorg, this chain segment is empty.
        new: Arc<Chain<N>>
    }
}
