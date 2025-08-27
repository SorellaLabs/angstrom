//! Flashblocks

use std::{
    collections::{HashMap, HashSet},
    ops::RangeInclusive
};

use alloy::{consensus::TxReceipt, eips::eip2718::Decodable2718};
use alloy_primitives::{Address, BlockHash, BlockNumber, TxHash, U256};
use reth::rpc::types::engine::{ExecutionPayloadV1, ExecutionPayloadV2, ExecutionPayloadV3};
use reth_optimism_primitives::{OpBlock, OpPrimitives, OpReceipt, OpTransactionSigned};
use reth_primitives::RecoveredBlock;
use reth_primitives_traits::Block;
use rollup_boost::FlashblocksPayloadV1;
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
pub struct PendingChain {
    /// The Flashblock payload.
    flashblock:   FlashblocksPayloadV1,
    /// The Flashblock as a recovered block.
    block:        RecoveredBlock<OpBlock>,
    /// The Flashblock metadata.
    metadata:     Metadata,
    /// The decoded transactions in the Flashblock.
    transactions: HashMap<TxHash, OpTransactionSigned>
}

impl PendingChain {
    /// Creates a new pending chain from a Flashblock payload. Decodes the
    /// metadata and the transactions.
    pub fn new(flashblock: FlashblocksPayloadV1) -> Self {
        let metadata = serde_json::from_value(flashblock.metadata.clone()).unwrap();
        let transactions = flashblock
            .diff
            .transactions
            .iter()
            .map(|tx| {
                let decoded = OpTransactionSigned::decode_2718(&mut tx.as_ref()).unwrap();
                (decoded.tx_hash(), decoded)
            })
            .collect();

        let base = flashblock.base.clone().expect("Base block is required");

        // Build the actual execution payload and block.
        let execution_payload = ExecutionPayloadV3 {
            blob_gas_used:   0,
            excess_blob_gas: 0,
            payload_inner:   ExecutionPayloadV2 {
                withdrawals:   flashblock.diff.withdrawals.clone(),
                payload_inner: ExecutionPayloadV1 {
                    parent_hash:      base.parent_hash,
                    fee_recipient:    base.fee_recipient,
                    state_root:       flashblock.diff.state_root,
                    receipts_root:    flashblock.diff.receipts_root,
                    logs_bloom:       flashblock.diff.logs_bloom,
                    prev_randao:      base.prev_randao,
                    block_number:     base.block_number,
                    gas_limit:        base.gas_limit,
                    gas_used:         flashblock.diff.gas_used,
                    timestamp:        base.timestamp,
                    extra_data:       base.extra_data,
                    base_fee_per_gas: base.base_fee_per_gas,
                    block_hash:       flashblock.diff.block_hash,
                    transactions:     flashblock.diff.transactions.clone()
                }
            }
        };

        let block = execution_payload
            .try_into_block()
            .expect("Failed to convert to block");

        let block = block.try_into_recovered().expect("Failed to recover block");

        Self { flashblock, block, metadata, transactions }
    }
}

/// We only implement this for `OpPrimitives` because that's the only scenario
/// where we have Flashblocks.
impl ChainExt<OpPrimitives> for PendingChain {
    /// Returns the block number of the canonical base block (not the
    /// Flashblock).
    fn tip_number(&self) -> BlockNumber {
        self.metadata.block_number
    }

    /// The index of the Flashblock in the block.
    fn flashblock_index(&self) -> Option<u64> {
        Some(self.flashblock.index)
    }

    /// Returns the block hash of the Flashblock.
    fn tip_hash(&self) -> BlockHash {
        self.flashblock.diff.block_hash
    }

    /// Returns the receipts for a given block hash.
    fn receipts_by_block_hash(&self, block_hash: BlockHash) -> Option<Vec<&OpReceipt>> {
        if self.tip_hash() == block_hash {
            Some(self.metadata.receipts.values().collect())
        } else {
            None
        }
    }

    fn tip_transactions(&self) -> impl Iterator<Item = &OpTransactionSigned> + '_ {
        self.transactions.values()
    }

    fn successful_tip_transactions(&self) -> impl Iterator<Item = &OpTransactionSigned> + '_ {
        let successful_hashes = self
            .metadata
            .receipts
            .iter()
            .filter_map(|(tx_hash, receipt)| receipt.status().then_some(tx_hash))
            .collect::<HashSet<_>>();

        self.transactions
            .iter()
            .filter_map(move |(tx_hash, tx)| successful_hashes.contains(tx_hash).then_some(tx))
    }

    /// Flashblocks are not reorged.
    /// TODO(mempirate): Is this actually the case?
    fn reorged_range(&self, _new: impl ChainExt<OpPrimitives>) -> Option<RangeInclusive<u64>> {
        None
    }

    fn blocks_iter(&self) -> impl Iterator<Item = &RecoveredBlock<OpBlock>> + '_ {
        vec![&self.block].into_iter()
    }
}
