use std::sync::Arc;

use alloy_rpc_types::Block;
use parking_lot::RwLock;
use reth_provider::{Chain, ExecutionOutcome};

#[derive(Clone, Debug)]
pub struct AnvilConsensusCanonStateNotification {
    chain: Arc<RwLock<Chain>>
}

impl AnvilConsensusCanonStateNotification {
    pub fn new() -> Self {
        Self { chain: Arc::new(RwLock::new(Chain::default())) }
    }

    pub fn new_block(&self, block: &Block) -> Arc<Chain> {
        let mut chain = self.chain.write();

        // the consensus only uses the block number so we can use default values for the
        // rest of the block
        let reth_block = reth_primitives::Block {
            header: reth_primitives::Header { number: block.header.number, ..Default::default() },
            ..Default::default()
        };

        chain.append_block(
            reth_block
                .with_recovered_senders()
                .unwrap()
                .seal(block.header.hash),
            ExecutionOutcome::default()
        );

        Arc::new(chain.clone())
    }
}
