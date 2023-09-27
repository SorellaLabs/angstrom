use bytes::{Bytes, BytesMut};
use reth_codecs::{main_codec, Compact};
use reth_primitives::{keccak256, H256, H512};
use reth_rlp::Encodable;
use secp256k1::PublicKey;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct RewardHeader {
    pub rewards_hash: H256,

    pub cumulative_lvr_bribe: u128,
    pub block_number:         u64,

    pub leader_pub_key: H512,
    pub attestations:   Vec<H512>
}

impl RewardHeader {
    pub fn new(block_number: u64, bribe: u128, leader: H512, attestations: Vec<H512>) -> Self {
        let mut buf = BytesMut::new();
        block_number.encode(&mut buf);
        bribe.encode(&mut buf);
        leader.encode(&mut buf);
        attestations.encode(&mut buf);
        let hash = keccak256(&buf.freeze()[..]);

        Self {
            rewards_hash: hash,
            attestations,
            block_number,
            cumulative_lvr_bribe: bribe,
            leader_pub_key: leader
        }
    }
}
