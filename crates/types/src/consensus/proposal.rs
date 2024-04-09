use alloy_rlp::Encodable;
use alloy_rlp_derive::{RlpDecodable, RlpEncodable};
use bytes::BytesMut;
use reth_primitives::keccak256;
use secp256k1::SecretKey;
use serde::{Deserialize, Serialize};

use crate::{
    consensus::order_buffer::OrderBuffer,
    primitive::{
        Angstrom::{Bundle, LowerBound},
        Signature
    }
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, RlpEncodable, RlpDecodable)]
pub struct Proposal {
    pub ethereum_block:   u64,
    pub vanilla_bundle:   Bundle,
    pub lower_bound:      LowerBound,
    pub order_buffer:     Vec<OrderBuffer>,
    /// This signature is over (etheruem_block | hash(vanilla_bundle) |
    /// hash(order_buffer) | hash(lower_bound))
    pub leader_signature: Signature
}

impl Proposal {
    pub fn generate_proposal(
        ethereum_block: u64,
        vanilla_bundle: Bundle,
        lower_bound: LowerBound,
        order_buffer: Vec<OrderBuffer>,
        sk: &SecretKey
    ) -> Self {
        // TODO: move to different to-byte conversion
        let mut buf = BytesMut::new();
        ethereum_block.encode(&mut buf);
        vanilla_bundle.encode(&mut buf);
        order_buffer.encode(&mut buf);
        lower_bound.encode(&mut buf);
        let buf = buf.freeze();

        let hash = keccak256(buf);
        let sig = reth_primitives::sign_message(sk.secret_bytes().into(), hash).unwrap();

        Self {
            ethereum_block,
            lower_bound,
            order_buffer,
            vanilla_bundle,
            leader_signature: Signature(sig)
        }
    }
}
