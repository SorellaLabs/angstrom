use alloy::{
    primitives::{BlockNumber, U256},
    signers::{Signature, SignerSync}
};
use alloy_primitives::keccak256;
use bytes::Bytes;
use itertools::Itertools;
use serde::{Deserialize, Serialize};

use super::{PreProposal, PreProposalAggregation};
use crate::{
    orders::PoolSolution,
    primitive::{AngstromSigner, PeerId}
};

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    // Might not be necessary as this is encoded in all the proposals anyways
    pub block_height: BlockNumber,
    pub source:       PeerId,
    /// PreProposals sorted by source
    pub preproposals: Vec<PreProposalAggregation>,
    /// PoolSolutions sorted by PoolId
    pub solutions:    Vec<PoolSolution>,
    /// This signature is over (etheruem_block | hash(vanilla_bundle) |
    /// hash(order_buffer) | hash(lower_bound))
    pub signature:    Signature
}

impl Default for Proposal {
    fn default() -> Self {
        Self {
            block_height: Default::default(),
            source:       Default::default(),
            preproposals: Default::default(),
            solutions:    Default::default(),
            signature:    Signature::new(U256::ZERO, U256::ZERO, false)
        }
    }
}

impl Proposal {
    pub fn generate_proposal(
        ethereum_height: BlockNumber,
        sk: &AngstromSigner,
        preproposals: Vec<PreProposalAggregation>,
        mut solutions: Vec<PoolSolution>
    ) -> Self {
        // Sort our solutions
        solutions.sort_by_key(|sol| sol.id);

        // Build our hash and sign
        let mut buf = Vec::new();
        buf.extend(bincode::serialize(&ethereum_height).unwrap());
        buf.extend(&sk.id());
        buf.extend(bincode::serialize(&preproposals).unwrap());
        buf.extend(bincode::serialize(&solutions).unwrap());
        let hash = keccak256(buf);
        let sig = sk.sign_hash_sync(&hash).unwrap();

        Self {
            block_height: ethereum_height,
            source: sk.id(),
            preproposals,
            solutions,
            signature: sig
        }
    }

    pub fn preproposals(&self) -> &Vec<PreProposalAggregation> {
        &self.preproposals
    }

    pub fn is_valid(&self, ethereum_height: &BlockNumber) -> bool {
        // All our preproposals have to be valid
        if !self
            .preproposals
            .iter()
            .all(|i| i.is_valid(ethereum_height))
        {
            return false
        }
        // Then our own signature has to be valid
        let hash = keccak256(self.payload());
        let Ok(source) = self.signature.recover_from_prehash(&hash) else {
            return false;
        };
        let source = AngstromSigner::public_key_to_peer_id(&source);

        source == self.source
    }

    fn payload(&self) -> Bytes {
        let mut buf = vec![];
        buf.extend(bincode::serialize(&self.block_height).unwrap());
        buf.extend(*self.source);
        buf.extend(bincode::serialize(&self.preproposals).unwrap());
        buf.extend(bincode::serialize(&self.solutions).unwrap());

        Bytes::from_iter(buf)
    }

    pub fn flattened_pre_proposals(&self) -> Vec<PreProposal> {
        self.preproposals
            .iter()
            .flat_map(|preproposal| preproposal.pre_proposals.clone())
            .unique_by(|proposal| proposal.source)
            .collect::<Vec<_>>()
    }
}

#[cfg(test)]
mod tests {
    use super::Proposal;
    use crate::primitive::AngstromSigner;

    #[test]
    fn can_be_constructed() {
        let ethereum_height = 100;
        let preproposals = vec![];
        let solutions = vec![];
        let sk = AngstromSigner::random();
        Proposal::generate_proposal(ethereum_height, &sk, preproposals, solutions);
    }

    #[test]
    fn can_validate_self() {
        let ethereum_height = 100;
        let preproposals = vec![];
        let solutions = vec![];
        // Generate crypto stuff
        let sk = AngstromSigner::random();
        let proposal = Proposal::generate_proposal(ethereum_height, &sk, preproposals, solutions);

        assert!(proposal.is_valid(&ethereum_height), "Unable to validate self");
    }
}
