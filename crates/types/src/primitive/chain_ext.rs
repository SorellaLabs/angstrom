use std::ops::RangeInclusive;

use alloy::{
    consensus::{BlockHeader, TxReceipt},
    primitives::{BlockHash, BlockNumber}
};
use reth_primitives::{EthPrimitives, NodePrimitives};
use reth_primitives_traits::{BlockBody, RecoveredBlock};
use reth_provider::Chain;

pub const MAX_REORG_DEPTH: u64 = 150;

#[auto_impl::auto_impl(&, Arc)]
pub trait ChainExt<N: NodePrimitives = EthPrimitives> {
    fn tip_number(&self) -> BlockNumber;
    fn tip_hash(&self) -> BlockHash;
    fn receipts_by_block_hash(&self, block_hash: BlockHash) -> Option<Vec<&N::Receipt>>;
    fn tip_transactions(&self) -> impl Iterator<Item = &N::SignedTx> + '_;
    fn successful_tip_transactions(&self) -> impl Iterator<Item = &N::SignedTx> + '_;
    fn reorged_range(&self, new: impl ChainExt<N>) -> Option<RangeInclusive<u64>>;
    fn blocks_iter(&self) -> impl Iterator<Item = &RecoveredBlock<N::Block>> + '_;
}

impl<N: NodePrimitives> ChainExt<N> for Chain<N> {
    fn tip_number(&self) -> BlockNumber {
        self.tip().number()
    }

    fn tip_hash(&self) -> BlockHash {
        self.tip().hash()
    }

    fn receipts_by_block_hash(&self, block_hash: BlockHash) -> Option<Vec<&N::Receipt>> {
        Chain::receipts_by_block_hash(self, block_hash)
    }

    fn successful_tip_transactions(&self) -> impl Iterator<Item = &N::SignedTx> + '_ {
        let execution = self
            .execution_outcome_at_block(self.tip().number())
            .unwrap();
        let receipts = execution.receipts.last().unwrap().clone();

        self.tip_transactions()
            .zip(receipts)
            .filter_map(|(tx, receipt)| receipt.status().then_some(tx))
    }

    fn tip_transactions(&self) -> impl Iterator<Item = &N::SignedTx> + '_ {
        self.tip().body().transactions().iter()
    }

    fn reorged_range(&self, new: impl ChainExt<N>) -> Option<RangeInclusive<u64>> {
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

    fn blocks_iter(&self) -> impl Iterator<Item = &RecoveredBlock<N::Block>> + '_ {
        self.blocks_iter()
    }
}
