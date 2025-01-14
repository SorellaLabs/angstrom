use std::{
    collections::{HashMap, HashSet},
    hash::Hasher
};

use alloy::{
    primitives::{keccak256, BlockNumber},
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
        grouped_orders::{GroupedVanillaOrder, OrderWithStorageData},
        rpc_orders::TopOfBlockOrder
    }
};

#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct PreProposal {
    pub block_height: BlockNumber,
    pub source:       PeerId,
    // TODO: this really should be HashMap<PoolId, GroupedVanillaOrder>
    pub limit:        Vec<OrderWithStorageData<GroupedVanillaOrder>>,
    // TODO: this really should be another type with HashMap<PoolId, {order, tob_reward}>
    pub searcher:     Vec<OrderWithStorageData<TopOfBlockOrder>>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreProposalContent {
    pub block_height: BlockNumber,
    pub source:       PeerId,
    pub limit:        Vec<OrderWithStorageData<GroupedVanillaOrder>>,
    pub searcher:     Vec<OrderWithStorageData<TopOfBlockOrder>>
}

// the reason for the manual implementation is because EcDSA signatures are not
// deterministic. EdDSA ones are, but the below allows for one less footgun
// If the struct switches to BLS, or any type of multisig or threshold
// signature, then the implementation should be changed to include it
impl std::hash::Hash for PreProposalContent {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.block_height.hash(state);
        self.source.hash(state);
        self.limit.hash(state);
        self.searcher.hash(state);
    }
}

impl PreProposal {
    pub fn content(&self) -> PreProposalContent {
        PreProposalContent {
            block_height: self.block_height,
            source:       self.source,
            limit:        self.limit.clone(),
            searcher:     self.searcher.clone()
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
        limit: Vec<OrderWithStorageData<GroupedVanillaOrder>>,
        searcher: Vec<OrderWithStorageData<TopOfBlockOrder>>
    ) -> Self {
        let payload = Self::serialize_payload(&ethereum_height, &limit, &searcher);
        let signature = Self::sign_payload(sk, payload);

        Self { limit, source: sk.id(), searcher, block_height: ethereum_height, signature }
    }

    pub fn new(
        ethereum_height: u64,
        sk: &AngstromSigner,
        orders: OrderSet<GroupedVanillaOrder, TopOfBlockOrder>
    ) -> Self {
        let OrderSet { limit, searcher } = orders;
        let limit_orders = limit.len();
        let searcher_orders = searcher.len();
        tracing::info!(%limit_orders,%searcher_orders, %ethereum_height,"building my pre_proposal");
        Self::generate_pre_proposal(ethereum_height, sk, limit, searcher)
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

    fn serialize_payload(
        block_height: &BlockNumber,
        limit: &Vec<OrderWithStorageData<GroupedVanillaOrder>>,
        searcher: &Vec<OrderWithStorageData<TopOfBlockOrder>>
    ) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend(bincode::serialize(block_height).unwrap());
        buf.extend(bincode::serialize(limit).unwrap());
        buf.extend(bincode::serialize(searcher).unwrap());
        buf
    }

    fn payload(&self) -> Bytes {
        Bytes::from(Self::serialize_payload(&self.block_height, &self.limit, &self.searcher))
    }

    pub fn orders_by_pool_id(
        preproposals: &[PreProposal]
    ) -> HashMap<PoolId, HashSet<OrderWithStorageData<GroupedVanillaOrder>>> {
        preproposals
            .iter()
            .flat_map(|p| p.limit.iter())
            .cloned()
            .fold(HashMap::new(), |mut acc, order| {
                acc.entry(order.pool_id).or_default().insert(order);
                acc
            })
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
