use alloy_primitives::B512;
use angstrom_types::{
    consensus::{Commit, PreProposal, Proposal},
    primitive::{
        Angstrom::{Bundle, LowerBound},
        BLSValidatorID
    }
};
use blsful::{Bls12381G1Impl, SecretKey};

/// The Signer deals with verifying external signatures as well as
/// signing our payloads.  Pub fields for now.
#[derive(Default)]
pub struct Signer {
    pub validator_id: BLSValidatorID,
    pub key:          SecretKey<Bls12381G1Impl>
}

impl Signer {
    #[allow(dead_code)]
    pub fn sign_proposal(
        &self,
        ethereum_block: u64,
        vanilla_bundle: Bundle,
        lower_bound: LowerBound
    ) -> eyre::Result<Proposal> {
        Ok(Proposal::generate_proposal(
            ethereum_block,
            vanilla_bundle,
            lower_bound,
            vec![],
            self.validator_id,
            &self.key
        ))
    }

    #[allow(dead_code)]
    pub fn sign_commit(&self, _ethereum_block: u64, proposal: &Proposal) -> eyre::Result<Commit> {
        Ok(Commit::from_proposal(proposal, &self.key))
    }

    #[allow(dead_code)]
    pub fn sign_pre_propose(
        &self,
        _ethereum_block: u64,
        _bundle: Bundle
    ) -> eyre::Result<PreProposal> {
        todo!()
    }

    pub fn is_us(&self, _addr: &B512) -> bool {
        todo!("change key to proper")
    }
}
