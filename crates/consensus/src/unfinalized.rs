use std::collections::VecDeque;

use guard_types::consensus::Block;

pub struct UnfinalizedBlockQueue(VecDeque<Block>);

impl UnfinalizedBlockQueue {
    pub fn new() -> Self {
        UnfinalizedBlockQueue(VecDeque::default())
    }

    pub fn on_new_block(&mut self, block: Block) {
        self.0.push_back(block);
    }

    pub fn new_finalized_ethereum_block(&mut self, eth_block: ()) -> Option<Block> {
        todo!()
    }
}
