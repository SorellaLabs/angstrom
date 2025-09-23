use alloy::{
    primitives::{Address, BlockNumber, U256, keccak256},
    signers::Signature
};
use alloy_primitives::B256;
use bytes::Bytes;
use reth_network_peers::PeerId;
use serde::{Deserialize, Serialize};

use crate::{
    consensus::PreProposal,
    primitive::{AngstromMetaSigner, AngstromSigner, public_key_to_peer_id}
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct PreProposalAggregation {
    pub block_height:  BlockNumber,
    pub source:        PeerId,
    pub pre_proposals: Vec<PreProposal>,
    pub signature:     Signature
}

impl Default for PreProposalAggregation {
    fn default() -> Self {
        Self {
            block_height:  Default::default(),
            source:        Default::default(),
            pre_proposals: Default::default(),
            signature:     Signature::new(U256::ZERO, U256::ZERO, false)
        }
    }
}

impl PreProposalAggregation {
    pub fn new<S: AngstromMetaSigner>(
        block_height: BlockNumber,
        sk: &AngstromSigner<S>,
        pre_proposals: Vec<PreProposal>
    ) -> Self {
        let payload = Self::serialize_payload(&block_height, &pre_proposals);
        let signature = Self::sign_payload(sk, payload);
        Self { block_height, source: sk.id(), pre_proposals, signature }
    }

    fn sign_payload<S: AngstromMetaSigner>(sk: &AngstromSigner<S>, payload: Vec<u8>) -> Signature {
        let hash = keccak256(payload);

        sk.sign_hash_sync(&hash).unwrap()
    }

    fn serialize_payload(block_height: &BlockNumber, pre_proposals: &[PreProposal]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend(bincode::serialize(block_height).unwrap());
        buf.extend(bincode::serialize(pre_proposals).unwrap());
        buf
    }

    fn payload(&self) -> Bytes {
        Bytes::from(Self::serialize_payload(&self.block_height, &self.pre_proposals))
    }

    pub fn recover_signer(&self) -> Option<Address> {
        let hash = keccak256(self.payload());
        self.signature.recover_address_from_prehash(&hash).ok()
    }

    pub fn is_valid(&self, block_height: &BlockNumber, two_thrids_th: usize) -> bool {
        if !self
            .pre_proposals
            .iter()
            .all(|prop| prop.is_valid(block_height))
        {
            return false;
        }

        if self.pre_proposals.len() < two_thrids_th {
            tracing::info!("got a pre_proposal aggregation with less than 2/3 of pre proposals");
            return false;
        }

        let hash = keccak256(self.payload());
        let Ok(source) = self.signature.recover_from_prehash(&hash) else {
            return false;
        };
        let source = public_key_to_peer_id(&source);

        source == self.source
    }

    pub fn searcher_order_hashes(&self) -> Vec<B256> {
        self.pre_proposals
            .iter()
            .flat_map(PreProposal::searcher_order_hashes)
            .collect()
    }

    pub fn limit_order_hashes(&self) -> Vec<B256> {
        self.pre_proposals
            .iter()
            .flat_map(PreProposal::limit_order_hashes)
            .collect()
    }
}
