use std::sync::Arc;

use alloy_rpc_types::Block;
use parking_lot::RwLock;
use reth_node_types::Block as _;
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
        let mut chain = self.chain.write();

        // the consensus only uses the block number so we can use default values for the
        // rest of the block
        let recovered = reth_primitives::Block {
            header: reth_primitives::Header { number: block.header.number, ..Default::default() },
            ..Default::default()
        }
        .try_into_recovered()
        .unwrap();

        chain.append_block(recovered, ExecutionOutcome::default());

        Arc::new(chain.clone())
    }
}
