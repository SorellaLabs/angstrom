use alloy_primitives::{Keccak256, B256};
use alloy_rlp_derive::{RlpDecodable, RlpEncodable};
use secp256k1::SecretKey;
use serde::{Deserialize, Serialize};

use crate::primitive::Signature;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, RlpEncodable, RlpDecodable)]
pub struct Commit {
    pub block_height: u64,

    pub vanilla_bundle_hash: B256,
    pub lower_bound_hash:    B256,
    pub order_buffer_hash:   B256,
    /// This signature is (block_height | vanilla_bundle_hash |
    /// lower_bound_hash | order_buffer_hash)
    pub message_sig:         Signature,
    /// is default if none. We have to due this due to the rlp requirements
    pub vanilla_bundle_sig:  Signature,
    /// is default if none. We have to due this due to the rlp requirements
    pub lower_bound_sig:     Signature,
    /// is default if none. We have to due this due to the rlp requirements
    pub order_buffer_sig:    Signature
}

impl Commit {
    pub fn generate_commit_all(
        block_height: u64,
        vanilla_bundle_hash: B256,
        lower_bound_hash: B256,
        order_buffer_hash: B256,
        sk: &SecretKey
    ) -> Self {
        let mut hasher = Keccak256::new();
        hasher.update(block_height.to_be_bytes());
        hasher.update(vanilla_bundle_hash);
        hasher.update(lower_bound_hash);
        hasher.update(order_buffer_hash);
        let message = hasher.finalize();

        let message_sig =
            Signature(reth_primitives::sign_message(sk.secret_bytes().into(), message).unwrap());

        let vanilla_bundle_sig = Signature(
            reth_primitives::sign_message(sk.secret_bytes().into(), vanilla_bundle_hash).unwrap()
        );
        let lower_bound_sig = Signature(
            reth_primitives::sign_message(sk.secret_bytes().into(), lower_bound_hash).unwrap()
        );
        let order_buffer_sig = Signature(
            reth_primitives::sign_message(sk.secret_bytes().into(), order_buffer_hash).unwrap()
        );
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
}
