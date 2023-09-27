use futures::executor::block_on;
// use guard_provider::traits::{GuardSetWriter, RewardsWriter, StateWriter};
use guard_types::{
    consensus::Block,
    database::{RewardHeader, State}
};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use reth_db::{
    database::Database,
    mdbx::{tx::Tx, Env, WriteMap, RO}
};
use reth_provider::{
    BlockReader, DatabaseProvider, LatestStateProvider, ReceiptProvider, StateProvider,
    TransactionsProvider
};
use tracing::info;

use crate::unfinalized::UnfinalizedQueue;

pub struct ChainMaintainer<T: Send + Sync> {
    reth_db:           Env<WriteMap>,
    unfinalized_queue: UnfinalizedQueue,
    provider:          T
}

impl<T: Send + Sync> ChainMaintainer<T>
// where
//     T: GuardSetWriter + RewardsWriter + StateWriter
{
    fn get_lastest_state_provider(&self) -> DatabaseProvider<'_, Tx<'_, RO, WriteMap>> {
        DatabaseProvider::new(self.reth_db.tx().unwrap(), Default::default())
    }

    pub fn on_new_finalized_eth_block(&mut self, block: u64) {
        let new_rewards = self
            .unfinalized_queue
            .new_finalized_ethereum_block(block)
            .into_par_iter()
            .filter_map(|(local_block, possible_rewards)| {
                let db = self.get_lastest_state_provider();
                let receipt = db
                    .receipt_by_hash(local_block.submitted_tx)
                    .map_err(|e| {
                        info!(?e, "receipt not found");
                        e
                    })
                    .ok()
                    .flatten()?;

                Some(possible_rewards).filter(|_| receipt.success)
            })
            .collect::<Vec<_>>();
    }

    fn insert_new_rewards(&mut self, rewards: Vec<RewardHeader>) {
        todo!()
    }

    fn derive_rewards(block: &Block) -> RewardHeader {
        let bribe = block.data.bundle.get_cumulative_lp_bribe();

        todo!();
        RewardHeader::new(
            block.header.height,
            bribe,
            block.header.proposer_address,
            //
            Vec::default()
        )
    }

    fn insert_block(&mut self, block: Block) {
        todo!()
    }

    fn insert_state(&mut self, state: State) {
        todo!()
    }

    pub fn on_new_height(&mut self, block: Block, state: State) {
        let rewards = Self::derive_rewards(&block);
        self.unfinalized_queue.0.push_back((block.clone(), rewards));

        self.insert_state(state);
        self.insert_block(block)
    }
}
