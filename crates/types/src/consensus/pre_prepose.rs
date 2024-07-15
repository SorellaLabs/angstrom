use alloy_rlp::Encodable;
use bytes::BytesMut;
use reth_primitives::keccak256;
use secp256k1::SecretKey;
use serde::{Deserialize, Serialize};

use crate::{
    orders::PooledOrder,
    primitive::{Angstrom::PoolKey, Signature},
    sol_bindings::grouped_orders::AllOrders
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct PoolOrders {
    pub pool:         PoolKey,
    pub searcher_bid: AllOrders,
    pub sorted_bids:  Vec<AllOrders>,
    pub sorted_asks:  Vec<AllOrders>
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct PreProposal {
    pub ethereum_height: u64,
    pub pre_bundle:      Vec<PoolOrders>,
    /// the signature is over the ethereum height and the bundle hash
    /// sign(ethereum_height | hash(pre_bundle))
    pub signature:       Signature
}

impl PreProposal {
    pub fn generate_pre_proposal(
        ethereum_height: u64,
        pre_bundle: Vec<PoolOrders>,
        sk: &SecretKey
    ) -> Self {
        let mut buf = BytesMut::new();
        ethereum_height.encode(&mut buf);
        let res = serde_json::to_vec(&pre_bundle).unwrap();
        buf.extend(res);

        let buf = buf.freeze();
        let hash = keccak256(buf);
        let sig = reth_primitives::sign_message(sk.secret_bytes().into(), hash).unwrap();

        Self { pre_bundle, ethereum_height, signature: Signature(sig) }
    }
}
