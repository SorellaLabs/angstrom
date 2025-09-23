use std::collections::HashSet;

use alloy_primitives::B256;

#[derive(Debug, Clone)]
pub struct ConsensusRoundOrderHashes {
    pub round:    ConsensusRoundEvent,
    pub limit:    HashSet<B256>,
    pub searcher: HashSet<B256>
}

#[derive(Debug, Clone, Copy)]
pub enum ConsensusRoundEvent {
    Noop,
    PropagatePreProposal,
    PropagatePreProposalAgg,
    PropagateProposal,
    PropagateEmptyBlockAttestation
}
