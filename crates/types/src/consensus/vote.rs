use serde::{Deserialize, Serialize};

use super::{BlockId, Time};
use crate::on_chain::Signature;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Vote {
    pub vote_type: VoteType,
    pub height:    u64,
    pub round:     u64,
    pub block_id:  Option<BlockId>,
    pub timestamp: Option<Time>,

    // /// Validator address
    // pub validator_address: account::Id,
    //
    // /// Validator index
    // pub validator_index: ValidatorIndex,
    /// Signature
    pub signature: Option<Signature>
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CanonicalVote {
    pub vote_type: VoteType,
    pub height:    u64,
    pub round:     u64,
    pub block_id:  Option<BlockId>,
    pub timestamp: Option<Time>,
    pub chain_id:  u64
}

/// SignedVote is the union of a canonicalized vote, the signature on
/// the sign bytes of that vote and the id of the validator who signed it.
pub struct SignedVote {
    pub vote:      CanonicalVote,
    // pub validator_address: account::Id,
    pub signature: Signature
}

impl SignedVote {
    /// Create new `SignedVote` from provided canonicalized vote, validator id,
    /// and the signature of that validator.
    pub fn new(
        vote: Vote,
        chain_id: u64,
        validator_address: account::Id,
        signature: Signature
    ) -> SignedVote {
        let canonical_vote = CanonicalVote::new(vote, chain_id);
        SignedVote { vote: canonical_vote, signature } //validator_address }
    }

    pub fn from_vote(vote: Vote, chain_id: u64) -> Option<Self> {
        let validator_address = vote.validator_address;
        vote.signature
            .clone()
            .map(|signature| Self::new(vote, chain_id, validator_address, signature))
    }

    /// Return the id of the validator that signed this vote.
    pub fn validator_id(&self) -> account::Id {
        self.validator_address
    }

    /// Return the actual signature on the canonicalized vote.
    pub fn signature(&self) -> &Signature {
        &self.signature
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum VoteType {
    Prevote   = 1,
    Precommit = 2
}
