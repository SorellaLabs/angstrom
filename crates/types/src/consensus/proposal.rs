use alloy_rlp::Encodable;
use alloy_rlp_derive::{RlpDecodable, RlpEncodable};
use bincode::{Decode, Encode};
use blsful::{Bls12381G1Impl, PublicKey, SecretKey, SignatureSchemes};
use bytes::{Bytes, BytesMut};
use serde::{Deserialize, Serialize};

use crate::{
    consensus::order_buffer::OrderBuffer,
    primitive::{
        Angstrom::{Bundle, LowerBound},
        BLSSignature, BLSValidatorID
    }
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Encode, Decode)]
pub struct Proposal {
    pub ethereum_block:   u64,
    pub vanilla_bundle:   Bundle,
    pub lower_bound:      LowerBound,
    pub order_buffer:     Vec<OrderBuffer>,
    /// This signature is over (etheruem_block | hash(vanilla_bundle) |
    /// hash(order_buffer) | hash(lower_bound))
    pub leader_signature: BLSSignature
}

impl Proposal {
    pub fn generate_proposal(
        ethereum_block: u64,
        vanilla_bundle: Bundle,
        lower_bound: LowerBound,
        order_buffer: Vec<OrderBuffer>,
        validator_id: BLSValidatorID,
        sk: &SecretKey<Bls12381G1Impl>
    ) -> Self {
        // TODO: move to different to-byte conversion
        let mut buf = BytesMut::new();
        ethereum_block.encode(&mut buf);
        vanilla_bundle.encode(&mut buf);
        order_buffer.encode(&mut buf);
        lower_bound.encode(&mut buf);
        let buf = buf.freeze();

        // This can only return an error and thusly a default (empty) signature if our
        // signing key is zero.  Highly unlikely but we should probably make
        // sure we think through this contingency
        let signature = sk
            .sign(SignatureSchemes::ProofOfPossession, &buf)
            .unwrap_or_default();
        let mut leader_signature = BLSSignature::default();
        leader_signature.add_signature(validator_id, signature);

        Self { ethereum_block, lower_bound, order_buffer, vanilla_bundle, leader_signature }
    }

    pub fn sign_proposal(
        &mut self,
        validator_id: BLSValidatorID,
        sk: &SecretKey<Bls12381G1Impl>
    ) -> bool {
        let Ok(signature) = sk.sign(SignatureSchemes::ProofOfPossession, &self.payload()) else {
            return false;
        };
        self.leader_signature.add_signature(validator_id, signature)
    }

    pub fn validate(&self, public_key_library: &[PublicKey<Bls12381G1Impl>]) -> bool {
        self.leader_signature
            .validate(public_key_library, &self.payload())
    }

    fn payload(&self) -> Bytes {
        let mut buf = BytesMut::new();
        self.ethereum_block.encode(&mut buf);
        self.vanilla_bundle.encode(&mut buf);
        self.order_buffer.encode(&mut buf);
        self.lower_bound.encode(&mut buf);
        buf.freeze()
    }
}
