use std::collections::HashMap;

use alloy_primitives::{
    aliases::{I24, U24},
    Address
};
use angstrom_types::{
    consensus::{PreProposal, Proposal},
    contract_bindings::angstrom::Angstrom::PoolKey,
    matching::{uniswap::LiqRange, SqrtPriceX96},
    primitive::PoolId,
    sol_bindings::{grouped_orders::OrderWithStorageData, rpc_orders::TopOfBlockOrder}
};
use matching_engine::{
    strategy::{MatchingStrategy, SimpleCheckpointStrategy},
    MatchingManager
};
use rand::thread_rng;
use reth_network_peers::pk2id;
use reth_tasks::TokioTaskExecutor;
use secp256k1::{Secp256k1, SecretKey as Secp256SecretKey};

use super::{pool::Pool, preproposal::PreproposalBuilder};
use crate::type_generator::{amm::AMMSnapshotBuilder, orders::SigningInfo};

#[derive(Debug, Default)]
pub struct ProposalBuilder {
    ethereum_height:   Option<u64>,
    order_count:       Option<usize>,
    preproposals:      Option<Vec<PreProposal>>,
    preproposal_count: Option<usize>,
    block:             Option<u64>,
    pools:             Option<Vec<Pool>>,
    sk:                Option<Secp256SecretKey>,
    order_key:         Option<SigningInfo>
}

impl ProposalBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn order_count(self, order_count: usize) -> Self {
        Self { order_count: Some(order_count), ..self }
    }

    pub fn preproposals(self, preproposals: Vec<PreProposal>) -> Self {
        Self { preproposals: Some(preproposals), ..self }
    }

    pub fn preproposal_count(self, preproposal_count: usize) -> Self {
        Self { preproposal_count: Some(preproposal_count), ..self }
    }

    pub fn for_block(self, block: u64) -> Self {
        Self { block: Some(block), ..self }
    }

    pub fn for_pools(self, pools: Vec<Pool>) -> Self {
        Self { pools: Some(pools), ..self }
    }

    pub fn for_random_pools(self, pool_count: usize) -> Self {
        let pools: Vec<Pool> = (0..pool_count)
            .map(|_| {
                let currency0 = Address::random();
                let currency1 = Address::random();
                let key = PoolKey {
                    currency0,
                    currency1,
                    fee: U24::ZERO,
                    tickSpacing: I24::unchecked_from(10),
                    hooks: Address::default()
                };
                let amm = AMMSnapshotBuilder::new(SqrtPriceX96::at_tick(100000).unwrap())
                    .with_positions(vec![
                        LiqRange::new(99000, 101000, 1_000_000_000_000_000_u128).unwrap()
                    ])
                    .build();
                Pool::new(key, amm, Address::random())
            })
            .collect();
        Self { pools: Some(pools), ..self }
    }

    pub fn with_secret_key(self, sk: Secp256SecretKey) -> Self {
        Self { sk: Some(sk), ..self }
    }

    pub fn order_key(self, order_key: Option<SigningInfo>) -> Self {
        Self { order_key, ..self }
    }

    pub fn build(self) -> Proposal {
        // Extract values from our struct
        let ethereum_height = self.ethereum_height.unwrap_or_default();
        let preproposal_count = self.preproposal_count.unwrap_or_default();
        let pools = self.pools.unwrap_or_default();
        let count = self.order_count.unwrap_or_default();
        let block = self.block.unwrap_or_default();
        let sk = self
            .sk
            .unwrap_or_else(|| Secp256SecretKey::new(&mut thread_rng()));
        // Build the source ID from the secret/public keypair
        let source = pk2id(&sk.public_key(&Secp256k1::new()));

        let preproposals = self.preproposals.unwrap_or_else(|| {
            (0..preproposal_count)
                .map(|_| {
                    PreproposalBuilder::new()
                        .for_block(block)
                        .order_count(count)
                        .for_pools(pools.clone())
                        .order_key(self.order_key.clone())
                        .build()
                })
                .collect::<Vec<_>>()
        });
        let books =
            MatchingManager::<TokioTaskExecutor>::build_books(&preproposals, HashMap::default());
        let searcher_orders: HashMap<PoolId, OrderWithStorageData<TopOfBlockOrder>> = preproposals
            .iter()
            .flat_map(|p| p.searcher.iter())
            .fold(HashMap::new(), |mut acc, order| {
                acc.entry(order.pool_id).or_insert(order.clone());
                acc
            });
        let solutions = books
            .into_iter()
            .map(|b| {
                let searcher = searcher_orders.get(&b.id()).cloned();
                SimpleCheckpointStrategy::run(&b)
                    .map(|s| s.solution(searcher))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        Proposal::generate_proposal(ethereum_height, source, preproposals, solutions, &sk)
    }
}
