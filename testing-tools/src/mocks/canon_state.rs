use std::sync::Arc;

use alloy_rpc_types::{Block, TransactionReceipt};
use itertools::Itertools;
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

    pub fn new_block(&self, block: &Block, receipts: Vec<TransactionReceipt>) -> Arc<Chain> {
        let mut chain = self.chain.write();

        // the consensus only uses the block number so we can use default values for the
        // rest of the block
        let b = block
            .clone()
            .into_consensus()
            .map_transactions(|tx| tx.into_recovered());

        let signers = b.body.transactions().map(|tx| tx.signer()).collect_vec();
        let block = block.clone().into_consensus().map_transactions(|t| {
            let signed = t.into_signed();
            let sig = *signed.signature();
            let raw_tx = signed.tx().clone();
            TransactionSigned::new_unhashed(raw_tx.into(), sig)
        });

        let recovered_block = RecoveredBlock::new_unhashed(block, signers);
        // recovered_block.
        // rec

        let mapped = receipts
            .into_iter()
            .map(|r| {
                let r = r.into_primitives_receipt();
                reth_primitives::Receipt {
                    tx_type:             r.inner.tx_type(),
                    success:             r.inner.status(),
                    cumulative_gas_used: r.inner.cumulative_gas_used(),
                    logs:                r.logs().to_vec()
                }
            })
            .collect_vec();
        let ex = ExecutionOutcome::default().with_receipts(vec![mapped]);
        if chain.execution_outcome().first_block() == 0 {
            chain
                .execution_outcome_mut()
                .set_first_block(recovered_block.number);
        }
        chain.append_block(recovered_block, ex);

        Arc::new(chain.clone())
    }
}
