use alloy_primitives::{FixedBytes, Keccak256, B256};
use alloy_rlp_derive::{RlpDecodable, RlpEncodable};
use bitmaps::Bitmap;
use blsful::{Bls12381G1Impl, PublicKey, SecretKey};
use serde::{Deserialize, Serialize};

use super::Proposal;
use crate::primitive::{BLSSignature, BLSValidatorID};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, RlpEncodable, RlpDecodable)]
pub struct Commit {
    pub block_height: u64,

    pub vanilla_bundle_hash: B256,
    pub lower_bound_hash:    B256,
    pub order_buffer_hash:   B256,
    /// This signature is (block_height | vanilla_bundle_hash |
    /// lower_bound_hash | order_buffer_hash)
    pub message_sig:         BLSSignature,
    /// is default if none. We have to due this due to the rlp requirements
    pub vanilla_bundle_sig:  BLSSignature,
    /// is default if none. We have to due this due to the rlp requirements
    pub lower_bound_sig:     BLSSignature,
    /// is default if none. We have to due this due to the rlp requirements
    pub order_buffer_sig:    BLSSignature
}

impl Commit {
    pub fn generate_commit_all(
        block_height: u64,
        vanilla_bundle_hash: B256,
        lower_bound_hash: B256,
        order_buffer_hash: B256,
        sk: &SecretKey<Bls12381G1Impl>
    ) -> Self {
        let mut hasher = Keccak256::new();
        hasher.update(block_height.to_be_bytes());
        hasher.update(vanilla_bundle_hash);
        hasher.update(lower_bound_hash);
        hasher.update(order_buffer_hash);
        let message = hasher.finalize();

        let message_sig = BLSSignature::sign(0, sk, message.as_slice());
        let vanilla_bundle_sig = BLSSignature::sign(0, sk, vanilla_bundle_hash.as_slice());
        let lower_bound_sig = BLSSignature::sign(0, sk, lower_bound_hash.as_slice());
        let order_buffer_sig = BLSSignature::sign(0, sk, order_buffer_hash.as_slice());

        Self {
            order_buffer_hash,
            lower_bound_hash,
            vanilla_bundle_hash,
            message_sig,
            order_buffer_sig,
            lower_bound_sig,
            vanilla_bundle_sig,
            block_height
        }
    }

    /// Get a reference to the validator bitmap for this Commit.  All the
    /// validator maps should always be the same so we return the one from
    /// `message_sig`
    pub fn validator_map(&self) -> &Bitmap<128> {
        self.message_sig.validator_map()
    }

    /// Returns the number of validators that have signed this Commit message
    pub fn num_signed(&self) -> usize {
        self.message_sig.validator_map().len()
    }

    fn hash_message(&self) -> FixedBytes<32> {
        let mut hasher = Keccak256::new();
        hasher.update(self.block_height.to_be_bytes());
        hasher.update(self.vanilla_bundle_hash);
        hasher.update(self.lower_bound_hash);
        hasher.update(self.order_buffer_hash);
        hasher.finalize()
    }

    pub fn add_signature(
        &mut self,
        validator_id: BLSValidatorID,
        sk: &SecretKey<Bls12381G1Impl>
    ) -> bool {
        // These can only fail if the SK is zero in which case they'll all fail so no
        // need to return early
        self.message_sig
            .sign_into(validator_id, sk, self.hash_message().as_slice());
        self.vanilla_bundle_sig
            .sign_into(validator_id, sk, self.vanilla_bundle_hash.as_slice());
        self.lower_bound_sig
            .sign_into(validator_id, sk, self.lower_bound_hash.as_slice());
        self.order_buffer_sig
            .sign_into(validator_id, sk, self.order_buffer_hash.as_slice())
    }

    pub fn validate(&self, public_key_library: &[PublicKey<Bls12381G1Impl>]) -> bool {
        self.message_sig
            .validate(public_key_library, self.hash_message().as_slice())
            && self
                .vanilla_bundle_sig
                .validate(public_key_library, self.vanilla_bundle_hash.as_slice())
            && self
                .lower_bound_sig
                .validate(public_key_library, self.lower_bound_hash.as_slice())
            && self
                .order_buffer_sig
                .validate(public_key_library, self.order_buffer_hash.as_slice())
    }

    /// Validate that this commit message is associated with a specific Proposal
    /// - incomplete
    pub fn is_for(&self, proposal: &Proposal) -> bool {
        self.block_height == proposal.ethereum_block
        // Also check to make sure our hashes match the proposal data
    }

    /// Returns true if this Commit claims to have been signed by the specified
    /// validator.  This does not inherently validate the Commit so make
    /// sure to do that as well!
    pub fn signed_by(&self, validator_id: BLSValidatorID) -> bool {
        self.message_sig.signed_by(validator_id)
            && self.vanilla_bundle_sig.signed_by(validator_id)
            && self.lower_bound_sig.signed_by(validator_id)
            && self.order_buffer_sig.signed_by(validator_id)
    }

    pub fn from_proposal(proposal: &Proposal, sk: &SecretKey<Bls12381G1Impl>) -> Self {
        let block_height = proposal.ethereum_block;
        let vanilla_bundle_hash = B256::random();
        let lower_bound_hash = B256::random();
        let order_buffer_hash = B256::random();

        Self::generate_commit_all(
            block_height,
            vanilla_bundle_hash,
            lower_bound_hash,
            order_buffer_hash,
            sk
        )
    }
}
