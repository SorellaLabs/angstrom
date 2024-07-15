use alloy_rlp::Encodable;
use bincode::{Decode, Encode};
use blsful::{Bls12381G1Impl, PublicKey, SecretKey, SignatureSchemes};
use bytes::{Bytes, BytesMut};
use reth_network_peers::PeerId;
use serde::{Deserialize, Serialize};

use super::PreProposal;
use crate::{
    orders::PoolSolution,
    primitive::{BLSSignature, BLSValidatorID}
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Encode, Decode)]
pub struct Proposal {
    // Might not be necessary as this is encoded in all the proposals anyways
    pub ethereum_height: u64,
    #[bincode(with_serde)]
    pub source:          PeerId,
    pub preproposals:    Vec<PreProposal>,
    pub solutions:       Vec<PoolSolution>,
    /// This signature is over (etheruem_block | hash(vanilla_bundle) |
    /// hash(order_buffer) | hash(lower_bound))
    pub signature:       BLSSignature
}

impl Proposal {
    pub fn generate_proposal(
        ethereum_height: u64,
        source: PeerId,
        preproposals: Vec<PreProposal>,
        solutions: Vec<PoolSolution>,
        validator_id: BLSValidatorID,
        sk: &SecretKey<Bls12381G1Impl>
    ) -> Self {
        // TODO: move to different to-byte conversion
        let mut buf = BytesMut::new();
        ethereum_height.encode(&mut buf);
        source.encode(&mut buf);
        preproposals.encode(&mut buf);
        solutions.encode(&mut buf);
        let buf = buf.freeze();

        // This can only return an error and thusly a default (empty) signature if our
        // signing key is zero.  Highly unlikely but we should probably make
        // sure we think through this contingency
        let signature = sk
            .sign(SignatureSchemes::ProofOfPossession, &buf)
            .unwrap_or_default();
        let mut leader_signature = BLSSignature::default();
        leader_signature.add_signature(validator_id, signature);

        Self { ethereum_height, source, preproposals, solutions, signature: leader_signature }
    }

    pub fn sign_proposal(
        &mut self,
        validator_id: BLSValidatorID,
        sk: &SecretKey<Bls12381G1Impl>
    ) -> bool {
        let Ok(signature) = sk.sign(SignatureSchemes::ProofOfPossession, &self.payload()) else {
            return false;
        };
        self.signature.add_signature(validator_id, signature)
    }

    pub fn validate(&self, public_key_library: &[PublicKey<Bls12381G1Impl>]) -> bool {
        self.signature.validate(public_key_library, &self.payload())
    }

    fn payload(&self) -> Bytes {
        let mut buf = BytesMut::new();
        self.ethereum_height.encode(&mut buf);
        self.source.encode(&mut buf);
        self.preproposals.encode(&mut buf);
        self.solutions.encode(&mut buf);
        buf.freeze()
    }
}
