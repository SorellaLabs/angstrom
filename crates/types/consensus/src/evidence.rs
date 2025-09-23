use thiserror::Error;

#[derive(Debug, Error)]
pub enum EvidenceError {
    #[error("invalid evidence")]
    InvalidEvidence
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Evidence {
    DuplicateVoteEvidence(DuplicateVoteEvidence)
}

/// Duplicate vote evidence
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DuplicateVoteEvidence {
    // pub vote_a:             Vote,
    // pub vote_b:             Vote,
    pub total_voting_power: u64,
    pub validator_power:    u64
}

impl DuplicateVoteEvidence {
    /// constructor
    pub fn new() -> Result<Self, EvidenceError> {
        Ok(Self { total_voting_power: Default::default(), validator_power: Default::default() })
    }
}
