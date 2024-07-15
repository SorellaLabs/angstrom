use alloy_rlp::Encodable;
use bincode::{Decode, Encode};
use bytes::BytesMut;
use reth_network_peers::PeerId;
use reth_primitives::keccak256;
use secp256k1::SecretKey;
use serde::{Deserialize, Serialize};

use crate::{
    orders::OrderSet,
    primitive::Signature,
    sol_bindings::{
        grouped_orders::{GroupedVanillaOrder, OrderWithStorageData},
        sol::TopOfBlockOrder
    }
};

// #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RlpDecodable,
// RlpEncodable)] pub struct PoolOrders {
//     pub pool:         PoolId,
//     pub searcher_bid: TopOfBlockOrder,
//     pub orders:       Vec<OrderWithStorageData<GroupedVanillaOrder>>
// }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
pub struct PreProposal {
    pub ethereum_height: u64,
    pub source:          PeerId,
    pub limit:           Vec<OrderWithStorageData<GroupedVanillaOrder>>,
    pub searcher:        Vec<OrderWithStorageData<TopOfBlockOrder>>,
    /// The signature is over the ethereum height as well as the limit and
    /// searcher sets
    pub signature:       Signature
}

impl PreProposal {
    pub fn generate_pre_proposal(
        ethereum_height: u64,
        source: PeerId,
        limit: Vec<OrderWithStorageData<GroupedVanillaOrder>>,
        searcher: Vec<OrderWithStorageData<TopOfBlockOrder>>,
        sk: &SecretKey
    ) -> Self {
        let mut buf = BytesMut::new();
        ethereum_height.encode(&mut buf);
        limit.encode(&mut buf);
        searcher.encode(&mut buf);
        let buf = buf.freeze();
        let hash = keccak256(buf);
        let sig = reth_primitives::sign_message(sk.secret_bytes().into(), hash).unwrap();

        Self { limit, source, searcher, ethereum_height, signature: Signature(sig) }
    }

    pub fn new(
        ethereum_height: u64,
        sk: &SecretKey,
        source: PeerId,
        orders: OrderSet<GroupedVanillaOrder, TopOfBlockOrder>
    ) -> Self {
        let OrderSet { limit, searcher } = orders;
        // orders
        //     .limit
        //     .keys()
        //     .cloned()
        //     .chain(orders.searcher.keys().cloned())
        //     .collect();
        // let pre_bundle: Vec<PoolOrders> = all_pools
        //     .iter()
        //     .map(|p| {
        //         // Gross that we have to reach in here because of the deref and clone
        // overlap         let searcher_bid =
        // orders.searcher.get(p).unwrap().clone();         let limit =
        // orders.limit.get(p);         let orders = limit.map(|m|
        // m.as_slice().into()).unwrap_or_default();         PoolOrders { pool:
        // *p, searcher_bid, orders }     })
        //     .collect();
        let mut buf = BytesMut::new();
        ethereum_height.encode(&mut buf);
        limit.encode(&mut buf);
        searcher.encode(&mut buf);
        let buf = buf.freeze();
        let hash = keccak256(buf);
        let sig = reth_primitives::sign_message(sk.secret_bytes().into(), hash).unwrap();
        Self { ethereum_height, source, limit, searcher, signature: Signature(sig) }
    }
}
