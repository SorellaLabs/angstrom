pub mod evidence;
pub mod pre_prepose;
pub mod pre_propose_agg;
pub mod proposal;

use alloy::primitives::{Address, BlockNumber, Bytes};
pub use evidence::*;
pub use pre_prepose::*;
pub use pre_propose_agg::*;
pub use proposal::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum StromConsensusEvent {
    PreProposal(Address, PreProposal),
    PreProposalAgg(Address, PreProposalAggregation),
    Proposal(Address, Proposal),
    BundleUnlockAttestation(Address, u64, Bytes)
}

impl StromConsensusEvent {
    pub fn message_type(&self) -> &'static str {
        match self {
            StromConsensusEvent::PreProposal(..) => "PreProposal",
            StromConsensusEvent::PreProposalAgg(..) => "PreProposalAggregation",
            StromConsensusEvent::Proposal(..) => "Proposal",
            StromConsensusEvent::BundleUnlockAttestation(..) => "BundleUnlockAttestation"
        }
    }

    pub fn sender(&self) -> Address {
        match self {
            StromConsensusEvent::PreProposal(peer_id, _)
            | StromConsensusEvent::Proposal(peer_id, _)
            | StromConsensusEvent::PreProposalAgg(peer_id, _)
            | StromConsensusEvent::BundleUnlockAttestation(peer_id, ..) => *peer_id
        }
    }

    pub fn block_height(&self) -> BlockNumber {
        match self {
            StromConsensusEvent::PreProposal(_, PreProposal { block_height, .. }) => *block_height,
            StromConsensusEvent::PreProposalAgg(_, p) => p.block_height,
            StromConsensusEvent::Proposal(_, Proposal { block_height, .. }) => *block_height,
            StromConsensusEvent::BundleUnlockAttestation(_, block, _) => *block
        }
    }
}
