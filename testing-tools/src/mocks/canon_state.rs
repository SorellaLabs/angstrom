use std::sync::Arc;

use alloy_rpc_types::Block;
use parking_lot::RwLock;
use reth_primitives::{RecoveredBlock, TransactionSigned};
use reth_provider::{Chain, ExecutionOutcome};

#[derive(Clone, Debug)]
pub struct AnvilConsensusCanonStateNotification {
    chain: Arc<RwLock<Chain>>
}
impl Default for AnvilConsensusCanonStateNotification {
    fn default() -> Self {
        Self::new()
    }
}

impl AnvilConsensusCanonStateNotification {
    pub fn new() -> Self {
        Self { chain: Arc::new(RwLock::new(Chain::default())) }
    }

    pub fn new_block(&self, block: &Block) -> Arc<Chain> {
        let mut recovered_block: RecoveredBlock<alloy::consensus::Block<TransactionSigned>> =
            RecoveredBlock::default();
        recovered_block.set_block_number(block.header.number);

        let mut chain = self.chain.write();
        chain.append_block(recovered_block, ExecutionOutcome::default());

        Arc::new(chain.clone())
    }
}
