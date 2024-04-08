use alloy_primitives::B512;
use angstrom_types::{
    consensus::{Commit, PreProposal, Proposal},
    primitive::Angstrom::{Bundle, LowerBound}
};
use ethers_signers::{LocalWallet, WalletError};
/// The Signer deals with verifying external signatures as well as
/// signing our payloads
#[repr(transparent)]
pub struct Signer(
    //TODO: this will prob change in future. placeholder for now
    LocalWallet
);

impl Signer {
    #[allow(dead_code)]
    pub fn sign_proposal(
        &self,
        _ethereum_block: u64,
        _vanilla_bundle: Bundle,
        _lower_bound: LowerBound
    ) -> Result<Proposal, WalletError> {
        todo!()
    }

    #[allow(dead_code)]
    pub fn sign_commit(
        &self,
        _ethereum_block: u64,
        _proposal: Proposal
    ) -> Result<Commit, WalletError> {
        todo!()
    }

    #[allow(dead_code)]
    pub fn sign_pre_propose(
        &self,
        _ethereum_block: u64,
        _bundle: Bundle
    ) -> Result<PreProposal, WalletError> {
        todo!()
    }

    pub fn is_us(&self, _addr: &B512) -> bool {
        todo!("change key to proper")
    }
}
