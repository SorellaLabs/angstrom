//! Flashblocks

use std::ops::RangeInclusive;

use alloy::consensus::TxReceipt;
use alloy_primitives::{BlockHash, BlockNumber};
use reth::rpc::server_types::eth::PendingBlock;
use reth_optimism_flashblocks::FlashBlockRx;
use reth_optimism_primitives::{OpBlock, OpPrimitives, OpReceipt, OpTransactionSigned};
use reth_primitives::RecoveredBlock;
use reth_primitives_traits::AlloyBlockHeader;
use tokio_stream::wrappers::WatchStream;

use crate::primitive::ChainExt;

mod db_wrapper;
mod provider;

/// A pending Flashblock.
pub type PendingFlashblock = PendingBlock<OpPrimitives>;

/// A type alias for the Flashblocks receiver.
pub type FlashblocksRx = FlashBlockRx<OpPrimitives>;

/// A stream of pending Flashblocks in the form of [`PendingFlashblock`]s.
pub type FlashblocksStream = WatchStream<Option<PendingFlashblock>>;

impl ChainExt<OpPrimitives> for PendingFlashblock {
    /// Returns the block number of the Flashblock (this will be the same as the
    /// base block number).
    fn tip_number(&self) -> BlockNumber {
        self.block().header().number()
    }

    /// Returns the block hash of the Flashblock (different for each
    /// Flashblock).
    fn tip_hash(&self) -> BlockHash {
        self.block().hash()
    }

    /// Returns the receipts for the Flashblock if the hash matches.
    fn receipts_by_block_hash(&self, block_hash: BlockHash) -> Option<Vec<&OpReceipt>> {
        if self.block().hash() == block_hash { Some(self.receipts.iter().collect()) } else { None }
    }

    /// Returns the transactions in this Flashblock.
    fn tip_transactions(&self) -> impl Iterator<Item = &OpTransactionSigned> + '_ {
        self.block().transactions_with_sender().map(|(_, tx)| tx)
    }

    /// Returns the successful transactions for the Flashblock tip.
    ///
    /// NOTE: In theory, this should just be all transactions since Flashblocks
    /// shouldn't contain reverts, but we filter here just to be safe.
    fn successful_tip_transactions(&self) -> impl Iterator<Item = &OpTransactionSigned> + '_ {
        // Zip transactions with receipts and filter by successful receipts
        self.tip_transactions()
            .zip(self.receipts.iter())
            .filter_map(|(tx, receipt)| receipt.status().then_some(tx))
    }

    /// Flashblocks are not reorged.
    fn reorged_range(&self, _new: impl ChainExt<OpPrimitives>) -> Option<RangeInclusive<u64>> {
        None
    }

    /// Returns an iterator over the Flashblock blocks (just one).
    fn blocks_iter(&self) -> impl Iterator<Item = &RecoveredBlock<OpBlock>> + '_ {
        std::iter::once(self.block().as_ref())
    }
}
