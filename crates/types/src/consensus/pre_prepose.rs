use std::{
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher}
};

use alloy::primitives::{keccak256, BlockNumber};
use bytes::Bytes;
use reth_network_peers::PeerId;
use secp256k1::SecretKey;
use serde::{Deserialize, Serialize};

use crate::{
    orders::OrderSet,
    primitive::{PoolId, Signature},
    sol_bindings::{
        grouped_orders::{GroupedVanillaOrder, OrderWithStorageData},
        rpc_orders::TopOfBlockOrder
    }
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
impl Hash for PreProposalContent {
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

impl Hash for PreProposal {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.content().hash(state);
    }
}

impl PartialEq for PreProposal {
    fn eq(&self, other: &Self) -> bool {
        self.content() == other.content()
    }
}

impl Eq for PreProposal {}

impl PreProposal {
    fn sign_payload(sk: &SecretKey, payload: Vec<u8>) -> Signature {
        let hash = keccak256(payload);
        let sig = reth_primitives::sign_message(sk.secret_bytes().into(), hash).unwrap();
        Signature(sig)
    }

    pub fn generate_pre_proposal(
        ethereum_height: BlockNumber,
        source: PeerId,
        limit: Vec<OrderWithStorageData<GroupedVanillaOrder>>,
        searcher: Vec<OrderWithStorageData<TopOfBlockOrder>>,
        sk: &SecretKey
    ) -> Self {
        let payload = Self::serialize_payload(&ethereum_height, &limit, &searcher);
        let signature = Self::sign_payload(sk, payload);

        Self { limit, source, searcher, block_height: ethereum_height, signature }
    }

    pub fn new(
        ethereum_height: u64,
        sk: &SecretKey,
        source: PeerId,
        orders: OrderSet<GroupedVanillaOrder, TopOfBlockOrder>
    ) -> Self {
        let OrderSet { limit, searcher } = orders;
        Self::generate_pre_proposal(ethereum_height, source, limit, searcher, sk)
    }

    pub fn is_valid(&self) -> bool {
        let hash = keccak256(self.payload());
        let Ok(source) = self.signature.recover_signer_full_public_key(hash) else {
            return false;
        };
        source == self.source
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
    use alloy::primitives::FixedBytes;
    use rand::thread_rng;
    use reth_network_peers::pk2id;
    use secp256k1::Secp256k1;

    use super::{PreProposal, SecretKey};

    #[test]
    fn can_be_constructed() {
        let ethereum_height = 100;
        let limit = vec![];
        let searcher = vec![];
        let source = FixedBytes::<64>::default();
        let sk = SecretKey::new(&mut rand::thread_rng());
        PreProposal::generate_pre_proposal(ethereum_height, source, limit, searcher, &sk);
    }

    #[test]
    fn can_validate_self() {
        let ethereum_height = 100;
        let limit = vec![];
        let searcher = vec![];
        // Generate crypto stuff
        let mut rng = thread_rng();
        let sk = SecretKey::new(&mut rng);
        let secp = Secp256k1::new();
        let pk = sk.public_key(&secp);
        // Grab the source ID from the secret/public keypair
        let source = pk2id(&pk);
        let preproposal =
            PreProposal::generate_pre_proposal(ethereum_height, source, limit, searcher, &sk);

        assert!(preproposal.is_valid(), "Unable to validate self");
    }
}
