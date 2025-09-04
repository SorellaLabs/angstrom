use std::sync::Arc;

use itertools::Itertools;
use op_alloy_consensus::{OpBlock, OpReceiptEnvelope};
use op_alloy_rpc_types::OpTransactionReceipt;
use parking_lot::RwLock;
use reth_optimism_primitives::{OpPrimitives, OpReceipt};
use reth_primitives::RecoveredBlock;
use reth_provider::{Chain, ExecutionOutcome};

use crate::types::OpRpcBlock;

#[derive(Debug, Clone)]
pub struct AnvilConsensusCanonStateNotification {
    chain: Arc<RwLock<Chain<OpPrimitives>>>
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

    pub fn current_block(&self) -> u64 {
        let chain = self.chain.read();
        chain.tip().number
    }

    pub fn new_block(
        &self,
        block: &OpRpcBlock,
        receipts: Vec<OpTransactionReceipt>
    ) -> Arc<Chain<OpPrimitives>> {
        let mut chain = self.chain.write();

        let block = block
            .clone()
            .into_consensus()
            .map_transactions(|t| t.inner.into_recovered());

        let recovered_block: RecoveredBlock<OpBlock> = block.into();

        let mapped = receipts
            .into_iter()
            .map(|r| {
                let r: OpReceiptEnvelope = r.into();
                r.into()
            })
            .collect_vec();

        let ex = ExecutionOutcome::<OpReceipt>::default().with_receipts(vec![mapped]);
        if chain.execution_outcome().first_block() == 0 {
            chain
                .execution_outcome_mut()
                .set_first_block(recovered_block.number);
        }

        chain.append_block(recovered_block, ex);

        Arc::new(chain.clone())
    }
}
