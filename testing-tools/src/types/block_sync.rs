use std::{ops::RangeInclusive, task::Waker};

use angstrom_types::block_sync::{BlockSyncConsumer, GlobalBlockState};

#[derive(Debug, Clone, Copy)]
pub struct MockBlockSync;

impl BlockSyncConsumer for MockBlockSync {
    fn sign_off_reorg(&self, _: &'static str, _: RangeInclusive<u64>, _: Option<Waker>) {}

    fn sign_off_on_block(&self, _: &'static str, _: u64, _: Option<Waker>) {}

    fn current_block_number(&self) -> u64 {
        0
    }

    fn has_proposal(&self) -> bool {
        false
    }

    fn fetch_current_proposal(&self) -> Option<GlobalBlockState> {
        None
    }

    fn register(&self, _: &'static str) {}
}
