use std::sync::Arc;

use alloy::consensus::BlockHeader;
use parking_lot::RwLock;
use reth_node_types::{Block as _, NodePrimitives};
use reth_provider::{Chain, ExecutionOutcome};

#[derive(Clone, Debug)]
pub struct AnvilConsensusCanonStateNotification<P: NodePrimitives> {
    chain: Arc<RwLock<Chain<P>>>
}
impl<P: NodePrimitives> Default for AnvilConsensusCanonStateNotification<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: NodePrimitives> AnvilConsensusCanonStateNotification<P> {
    pub fn new() -> Self {
        Self { chain: Arc::new(RwLock::new(Chain::default())) }
    }

    pub fn current_block(&self) -> u64 {
        let chain = self.chain.read();
        chain.tip().number()
    }

    pub fn new_block(&self, block: &P::Block, receipts: Vec<P::Receipt>) -> Arc<Chain<P>> {
        let mut chain = self.chain.write();

        // TOOD(havard): If something doesn't work, this has been changed quyite a lot.
        // Look at commented code. let b = block
        //     .into_ethereum_block()
        //     .map_transactions(|tx| tx.try_into_recovered().unwrap());

        // let signers = b.body.transactions().map(|tx| tx.signer()).collect_vec();

        let recovered = block.clone().try_into_recovered().unwrap();

        // let block = b.map_transactions(|t| {
        //     let signed = t.into_signed();
        //     let sig = *signed.signature();
        //     let raw_tx = signed.tx().clone();
        //     P::SignedTx::new_unchecked(raw_tx.into(), sig)
        // });

        // recovered_block.
        // rec

        // let mapped = receipts
        //     .into_iter()
        //     .map(|r| {
        //         let r = r.into();
        //         reth_primitives::Receipt {
        //             tx_type: r.inner.tx_type(),
        //             success: r.inner.status(),
        //             cumulative_gas_used: r.inner.cumulative_gas_used(),
        //             logs: r.logs().to_vec(),
        //         }
        //     })
        //     .collect_vec();
        let ex = ExecutionOutcome::<P::Receipt>::default().with_receipts(vec![receipts]);
        if chain.execution_outcome().first_block() == 0 {
            chain
                .execution_outcome_mut()
                .set_first_block(recovered.number());
        }

        chain.append_block(recovered, ex);

        Arc::new(chain.clone())
    }
}
