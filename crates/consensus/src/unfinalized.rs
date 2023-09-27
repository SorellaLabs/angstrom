use std::collections::VecDeque;

use guard_types::{consensus::Block, database::RewardHeader};

pub struct UnfinalizedQueue(pub VecDeque<(Block, RewardHeader)>);

impl UnfinalizedQueue {
    pub fn new() -> Self {
        UnfinalizedQueue(VecDeque::default())
    }

    pub fn on_new_block(&mut self, rewards: (Block, RewardHeader)) {
        self.0.push_back(rewards);
    }

    pub fn new_finalized_ethereum_block(
        &mut self,
        eth_height: u64
    ) -> Vec<(Block, RewardHeader)> {
        let mut res = Vec::new();
        while self
            .0
            .front()
            .filter(|b| b.0.header.ethereum_height <= eth_height)
            .is_some()
        {
            res.push(self.0.pop_front().unwrap())
        }

        res
    }
}
