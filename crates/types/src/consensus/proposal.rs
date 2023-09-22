use super::Time;
use crate::on_chain::Signature;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Proposal {
    pub proposal_type: ProposalType,
    pub height:        u64,
    pub round:         u64,
    pub pol_round:     u64,
    pub timestamp:     Time,
    pub signature:     Signature
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
pub enum ProposalType {
    Proposal = 32
}
