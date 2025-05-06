use std::collections::{HashMap, HashSet};

use alloy::{
    primitives::{B256, BlockNumber, keccak256},
    signers::{Signature, SignerSync}
};
use alloy_primitives::U256;
use bytes::Bytes;
use reth_network_peers::PeerId;
use serde::{Deserialize, Serialize};

use crate::{
    orders::OrderSet,
    primitive::{AngstromSigner, PoolId},
    sol_bindings::{
        ext::RawPoolOrder,
        grouped_orders::{AllOrders, OrderWithStorageData},
        rpc_orders::TopOfBlockOrder
    }
};

#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct PreProposal {
    pub block_height: BlockNumber,
    pub source:       PeerId,
    pub limit:        Vec<B256>,
    pub searcher:     Vec<B256>,
    /// The signature is over the ethereum height as well as the limit and
    /// searcher sets
    pub signature:    Signature
}

impl Default for PreProposal {
    fn default() -> Self {
        Self {
            signature:    Signature::new(U256::ZERO, U256::ZERO, false),
            block_height: Default::default(),
            source:       Default::default(),
            limit:        Default::default(),
            searcher:     Default::default()
        }
    }
}

impl PreProposal {
    fn sign_payload(sk: &AngstromSigner, payload: Vec<u8>) -> Signature {
        let hash = keccak256(payload);
        sk.sign_hash_sync(&hash).unwrap()
    }

    pub fn generate_pre_proposal(
        ethereum_height: BlockNumber,
        sk: &AngstromSigner,
        limit: Vec<B256>,
        searcher: Vec<B256>
    ) -> Self {
        let payload = Self::serialize_payload(&ethereum_height, &limit, &searcher);
        let signature = Self::sign_payload(sk, payload);

        Self { limit, source: sk.id(), searcher, block_height: ethereum_height, signature }
    }

    pub fn new(
        ethereum_height: u64,
        sk: &AngstromSigner,
        orders: OrderSet<AllOrders, TopOfBlockOrder>
    ) -> Self {
        let OrderSet { limit, searcher } = orders;
        let limit_orders = limit.len();
        let searcher_orders = searcher.len();
        tracing::info!(%limit_orders,%searcher_orders, %ethereum_height,"building my pre_proposal");
        let limit_hashes = limit.into_iter().map(|order| order.order_hash()).collect();
        let searcher_hashes = searcher
            .into_iter()
            .map(|order| order.order_hash())
            .collect();
        Self::generate_pre_proposal(ethereum_height, sk, limit_hashes, searcher_hashes)
    }

    /// ensures block height is correct as-well as validates the signature.
    pub fn is_valid(&self, block_height: &BlockNumber) -> bool {
        let hash = keccak256(self.payload());
        let Ok(source) = self.signature.recover_from_prehash(&hash) else {
            return false;
        };
        let source = AngstromSigner::public_key_to_peer_id(&source);

        source == self.source && &self.block_height == block_height
    }

    fn serialize_payload(block_height: &BlockNumber, limit: &[B256], searcher: &[B256]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend(bincode::serialize(block_height).unwrap());
        buf.extend(bincode::serialize(limit).unwrap());
        buf.extend(bincode::serialize(searcher).unwrap());
        buf
    }

    fn payload(&self) -> Bytes {
        Bytes::from(Self::serialize_payload(&self.block_height, &self.limit, &self.searcher))
    }
}

#[cfg(test)]
mod tests {

    use super::PreProposal;
    use crate::primitive::AngstromSigner;

    #[test]
    fn can_be_constructed() {
        let ethereum_height = 100;
        let limit = vec![];
        let searcher = vec![];
        let sk = AngstromSigner::random();
        PreProposal::generate_pre_proposal(ethereum_height, &sk, limit, searcher);
    }

    #[test]
    fn can_validate_self() {
        let ethereum_height = 100;
        let limit = vec![];
        let searcher = vec![];
        // Generate crypto stuff
        let sk = AngstromSigner::random();
        let preproposal = PreProposal::generate_pre_proposal(ethereum_height, &sk, limit, searcher);

        assert!(preproposal.is_valid(&ethereum_height), "Unable to validate self");
    }
}
