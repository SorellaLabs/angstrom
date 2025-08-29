//! Flashblocks

use std::{collections::HashMap, ops::RangeInclusive};

use alloy::{
    consensus::TxReceipt,
    rlp::{Decodable, Encodable}
};
use alloy_primitives::{Address, BlockHash, BlockNumber, TxHash, U256};
use reth::rpc::{
    server_types::eth::PendingBlock,
    types::engine::{ExecutionPayloadV1, ExecutionPayloadV2, ExecutionPayloadV3}
};
use reth_optimism_flashblocks::FlashBlockRx;
use reth_optimism_primitives::{OpBlock, OpPrimitives, OpReceipt, OpTransactionSigned};
use reth_primitives::{NodePrimitives, RecoveredBlock};
use reth_primitives_traits::{AlloyBlockHeader, Block};
pub use rollup_boost::{ExecutionPayloadBaseV1, FlashblocksPayloadV1};
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::WatchStream;

use crate::primitive::ChainExt;

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

/// Metadata for a Flashblock. This is the same for Base and Unichain.
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct Metadata<R: TxReceipt> {
    pub receipts:             HashMap<TxHash, R>,
    pub new_account_balances: HashMap<Address, U256>,
    pub block_number:         u64
}

/// A chain that is "pending", as in built from Flashblocks. Based on
/// [`Chain`](reth_provider::Chain) and implements
/// [`ChainExt`](crate::primitive::ChainExt).
///
/// A pending chain lasts for a single slot time, and consists of (slot time /
/// flashblock interval) blocks.
#[derive(Debug)]
pub struct PendingChain<N: NodePrimitives = OpPrimitives> {
    /// The Flashblock as a recovered block.
    blocks:    Vec<RecoveredBlock<N::Block>>,
    /// The Flashblock metadata. Index corresponds to the block index.
    metadatas: Vec<Metadata<N::Receipt>>,
    /// The base block of this pending chain.
    base:      ExecutionPayloadBaseV1
}

trait FlashblockExt {
    fn into_recovered_block<N: NodePrimitives>(
        &self,
        base: &ExecutionPayloadBaseV1
    ) -> RecoveredBlock<N::Block>;
}

impl FlashblockExt for FlashblocksPayloadV1 {
    fn into_recovered_block<N: NodePrimitives>(
        &self,
        base: &ExecutionPayloadBaseV1
    ) -> RecoveredBlock<N::Block> {
        // Build the actual execution payload and block.
        let execution_payload = ExecutionPayloadV3 {
            blob_gas_used:   0,
            excess_blob_gas: 0,
            payload_inner:   ExecutionPayloadV2 {
                withdrawals:   self.diff.withdrawals.clone(),
                payload_inner: ExecutionPayloadV1 {
                    parent_hash:      base.parent_hash,
                    fee_recipient:    base.fee_recipient,
                    state_root:       self.diff.state_root,
                    receipts_root:    self.diff.receipts_root,
                    logs_bloom:       self.diff.logs_bloom,
                    prev_randao:      base.prev_randao,
                    block_number:     base.block_number,
                    gas_limit:        base.gas_limit,
                    gas_used:         self.diff.gas_used,
                    timestamp:        base.timestamp,
                    extra_data:       base.extra_data.clone(),
                    base_fee_per_gas: base.base_fee_per_gas,
                    block_hash:       self.diff.block_hash,
                    transactions:     self.diff.transactions.clone()
                }
            }
        };

        let block = execution_payload
            .try_into_block::<N::SignedTx>()
            .expect("Failed to convert to block");

        // NOTE: This BS encode/decode is necessary to convert from concrete type to
        // the generic type N::Block.
        let mut buf = Vec::new();
        block.encode(&mut buf);

        let decoded_block = N::Block::decode(&mut buf.as_slice()).expect("Failed to decode block");

        decoded_block
            .try_into_recovered()
            .expect("Failed to recover block")
    }
}

// Specialized implementation for OpPrimitives
impl<N: NodePrimitives> PendingChain<N> {
    /// Creates a new pending chain from a Flashblock payload. Decodes the
    /// metadata and the transactions. This should only be used for the first
    /// Flashblock for a certain slot (i.e. index = 0), because it expects the
    /// base to be present.
    ///
    /// If you want to add a Flashblock to an existing pending chain, use
    /// [`Self::push_flashblock`] instead.
    ///
    /// # Panics
    /// Panics if the base block is not present.
    pub fn new(flashblock: FlashblocksPayloadV1) -> Self {
        let metadata = serde_json::from_value(flashblock.metadata.clone()).unwrap();

        let base = flashblock.base.clone().expect("Base block is required");

        // Build the actual execution payload and block.
        let block = flashblock.into_recovered_block::<N>(&base);

        // Capacity = 5 (1s / 200ms)
        let mut blocks = Vec::with_capacity(5);
        blocks.push(block);

        let mut metadatas = Vec::with_capacity(5);
        metadatas.push(metadata);

        Self { blocks, base, metadatas }
    }

    /// Pushes a new Flashblock to the pending chain. This will build the
    /// execution payload and block, and then recover the block.
    pub fn push_flashblock(&mut self, flashblock: FlashblocksPayloadV1) {
        let metadata = serde_json::from_value(flashblock.metadata.clone()).unwrap();

        let block = flashblock.into_recovered_block::<N>(&self.base);

        self.blocks.push(block);
        self.metadatas.push(metadata);
    }

    /// Returns the index of the tip block (last Flashblock).
    pub fn tip_index(&self) -> usize {
        self.blocks.len() - 1
    }

    /// Returns the tip block (last Flashblock).
    pub fn tip(&self) -> &RecoveredBlock<N::Block> {
        // SAFETY: There's always a block in the pending chain.
        self.blocks.last().unwrap()
    }

    /// Returns the base block of this pending chain.
    pub fn base(&self) -> &ExecutionPayloadBaseV1 {
        &self.base
    }
}

/// Generic implementation for any NodePrimitives - provides default behavior
impl<N: NodePrimitives> ChainExt<N> for PendingChain<N> {
    /// Returns the block number of the canonical base block (not the
    /// Flashblock).
    fn tip_number(&self) -> BlockNumber {
        self.metadatas.last().unwrap().block_number
    }

    /// Returns the block hash of the Flashblock.
    fn tip_hash(&self) -> BlockHash {
        self.blocks.last().unwrap().hash()
    }

    /// Returns the receipts for a given Flashblock block hash.
    fn receipts_by_block_hash(&self, block_hash: BlockHash) -> Option<Vec<&N::Receipt>> {
        let index = self
            .blocks
            .iter()
            .position(|block| block.hash() == block_hash)?;

        let metadata = &self.metadatas[index];

        Some(metadata.receipts.values().collect())
    }

    /// Returns the transactions for the Flashblock tip.
    fn tip_transactions(&self) -> impl Iterator<Item = &N::SignedTx> + '_ {
        self.blocks
            .last()
            .unwrap()
            .transactions_with_sender()
            .map(|(_, tx)| tx)
    }

    /// Returns the successful transactions for the Flashblock tip.
    ///
    /// NOTE: In theory, this should just be all transactions since Flashblocks
    /// shouldn't contain reverts, but we filter here just to be safe.
    fn successful_tip_transactions(&self) -> impl Iterator<Item = &N::SignedTx> + '_ {
        self.tip_transactions()
    }

    /// Flashblocks are not reorged.
    fn reorged_range(&self, _new: impl ChainExt<N>) -> Option<RangeInclusive<u64>> {
        None
    }

    /// Returns an iterator over the Flashblock blocks.
    fn blocks_iter(&self) -> impl Iterator<Item = &RecoveredBlock<N::Block>> + '_ {
        self.blocks.iter()
    }
}
