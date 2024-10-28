use alloy::primitives::{BlockNumber, FixedBytes};
use angstrom_types::{
    consensus::{PreProposal, Proposal},
    orders::PoolSolution,
    primitive::PeerId
};
use secp256k1::{rand::thread_rng, SecretKey};

/// The Signer deals with verifying external signatures as well as
/// signing our payloads.  Pub fields for now.
#[derive(Clone)]
pub struct Signer {
    pub my_id: PeerId,
    pub key:   SecretKey
}

impl Default for Signer {
    fn default() -> Self {
        let rng = thread_rng();
        let key = SecretKey::new(&mut secp256k1::rand::thread_rng());
        Signer { my_id: FixedBytes::random(), key }
    }
}

impl Signer {
    pub fn new(secret_key: SecretKey) -> Self {
        Self { key: secret_key, ..Default::default() }
    }

    pub fn sign_proposal(
        &self,
        ethereum_block: BlockNumber,
        preproposals: Vec<PreProposal>,
        solutions: Vec<PoolSolution>
    ) -> Proposal {
        Proposal::generate_proposal(ethereum_block, self.my_id, preproposals, solutions, &self.key)
    }
}
