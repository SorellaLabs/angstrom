use std::ops::RangeInclusive;

use alloy::{
    consensus::BlockHeader,
    primitives::{BlockHash, BlockNumber}
};
use reth_ethereum_primitives::{Block, Receipt, TransactionSigned};
use reth_primitives_traits::RecoveredBlock;
use reth_provider::Chain;

pub const MAX_REORG_DEPTH: u64 = 150;

#[auto_impl::auto_impl(&, Arc)]
pub trait ChainExt {
    fn tip_number(&self) -> BlockNumber;
    fn tip_hash(&self) -> BlockHash;
    fn receipts_by_block_hash(&self, block_hash: BlockHash) -> Option<Vec<&Receipt>>;
    fn tip_transactions(&self) -> impl Iterator<Item = &TransactionSigned> + '_;
    fn successful_tip_transactions(&self) -> impl Iterator<Item = &TransactionSigned> + '_;
    fn reorged_range(&self, new: impl ChainExt) -> Option<RangeInclusive<u64>>;
    fn blocks_iter(&self) -> impl Iterator<Item = &RecoveredBlock<Block>> + '_;
}

impl ChainExt for Chain {
    fn tip_number(&self) -> BlockNumber {
        self.tip().number
    }

    fn tip_hash(&self) -> BlockHash {
        self.tip().hash()
    }

    fn receipts_by_block_hash(&self, block_hash: BlockHash) -> Option<Vec<&Receipt>> {
        self.receipts_by_block_hash(block_hash)
    }

    fn successful_tip_transactions(&self) -> impl Iterator<Item = &TransactionSigned> + '_ {
        self.tip_transactions()
            .zip(
                self.receipts_by_block_hash(self.tip_hash())
                    .unwrap()
                    .into_iter()
            )
            .filter_map(|(tx, receipt)| receipt.success.then_some(tx))
    }

    fn tip_transactions(&self) -> impl Iterator<Item = &TransactionSigned> + '_ {
        self.tip().body().transactions.iter()
    }

    fn reorged_range(&self, new: impl ChainExt) -> Option<RangeInclusive<u64>> {
        let tip = new.tip_number();
        // search 150 blocks back;
        let start = tip - MAX_REORG_DEPTH;

        let mut range = self
            .blocks_iter()
            .filter(|b| b.number() >= start)
            .zip(new.blocks_iter().filter(|b| b.number() >= start))
            .filter(|&(old, new)| (old.hash() != new.hash()))
            .map(|(_, new)| new.number())
            .collect::<Vec<_>>();

        match range.len() {
            0 => None,
            1 => {
                let r = range.remove(0);
                Some(r..=r)
            }
            _ => {
                let start = *range.first().unwrap();
                let end = *range.last().unwrap();
                Some(start..=end)
            }
        }
    }

    fn blocks_iter(&self) -> impl Iterator<Item = &RecoveredBlock<Block>> + '_ {
        self.blocks_iter()
    }
}
