//! Flashblocks

use std::{
    collections::{HashMap, HashSet},
    ops::RangeInclusive
};

use alloy::consensus::TxReceipt;
use alloy_primitives::{Address, BlockHash, BlockNumber, TxHash, U256};
use reth::rpc::types::engine::{ExecutionPayloadV1, ExecutionPayloadV2, ExecutionPayloadV3};
use reth_optimism_primitives::{OpBlock, OpPrimitives, OpReceipt, OpTransactionSigned};
use reth_primitives::RecoveredBlock;
use reth_primitives_traits::Block;
use rollup_boost::ExecutionPayloadBaseV1;
pub use rollup_boost::FlashblocksPayloadV1;
use serde::{Deserialize, Serialize};

use crate::primitive::ChainExt;

/// Metadata for a Flashblock. This is the same for Base and Unichain.
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct Metadata {
    pub receipts:             HashMap<TxHash, OpReceipt>,
    pub new_account_balances: HashMap<Address, U256>,
    pub block_number:         u64
}

/// A chain that is "pending", as in built from Flashblocks. Based on
/// [`Chain`](reth_provider::Chain) and implements
/// [`ChainExt`](crate::primitive::ChainExt).
///
/// A pending chain lasts for a single slot time, and consists of (slot time /
/// flashblock interval) blocks.
pub struct PendingChain {
    /// The Flashblock as a recovered block.
    blocks:    Vec<RecoveredBlock<OpBlock>>,
    /// The Flashblock metadata. Index corresponds to the block index.
    metadatas: Vec<Metadata>,
    /// The base block of this pending chain.
    base:      ExecutionPayloadBaseV1
}

trait FlashblockExt {
    fn into_recovered_block(&self, base: &ExecutionPayloadBaseV1) -> RecoveredBlock<OpBlock>;
}

impl FlashblockExt for FlashblocksPayloadV1 {
    fn into_recovered_block(&self, base: &ExecutionPayloadBaseV1) -> RecoveredBlock<OpBlock> {
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
            .try_into_block()
            .expect("Failed to convert to block");

        block.try_into_recovered().expect("Failed to recover block")
    }
}

impl PendingChain {
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
        let block = flashblock.into_recovered_block(&base);

        // Capacity = 5 (1s / 250ms)
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

        let block = flashblock.into_recovered_block(&self.base);

        self.blocks.push(block);
        self.metadatas.push(metadata);
    }
}

/// We only implement this for `OpPrimitives` because that's the only scenario
/// where we have Flashblocks.
impl ChainExt<OpPrimitives> for PendingChain {
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
    fn receipts_by_block_hash(&self, block_hash: BlockHash) -> Option<Vec<&OpReceipt>> {
        let index = self
            .blocks
            .iter()
            .position(|block| block.hash() == block_hash)?;

        let metadata = &self.metadatas[index];

        Some(metadata.receipts.values().collect())
    }

    /// Returns the transactions for the Flashblock tip.
    fn tip_transactions(&self) -> impl Iterator<Item = &OpTransactionSigned> + '_ {
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
    fn successful_tip_transactions(&self) -> impl Iterator<Item = &OpTransactionSigned> + '_ {
        let successful_hashes = self
            .metadatas
            .last()
            .unwrap()
            .receipts
            .iter()
            .filter_map(|(tx_hash, receipt)| receipt.status().then_some(tx_hash))
            .collect::<HashSet<_>>();

        self.tip_transactions()
            .filter(move |tx| successful_hashes.contains(&tx.tx_hash()))
    }

    /// Flashblocks are not reorged.
    /// TODO(mempirate): Is this actually the case?
    fn reorged_range(&self, _new: impl ChainExt<OpPrimitives>) -> Option<RangeInclusive<u64>> {
        None
    }

    /// Returns an iterator over the Flashblock blocks.
    fn blocks_iter(&self) -> impl Iterator<Item = &RecoveredBlock<OpBlock>> + '_ {
        self.blocks.iter()
    }
}
