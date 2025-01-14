use std::collections::HashMap;

use alloy_primitives::{
    aliases::{I24, U24},
    Address
};
use angstrom_types::{
    consensus::{PreProposalAggregation, Proposal},
    contract_bindings::angstrom::Angstrom::PoolKey,
    matching::{uniswap::LiqRange, SqrtPriceX96},
    primitive::{AngstromSigner, PoolId},
    sol_bindings::{grouped_orders::OrderWithStorageData, rpc_orders::TopOfBlockOrder}
};
use matching_engine::{
    strategy::{MatchingStrategy, SimpleCheckpointStrategy},
    MatchingManager
};
use reth_tasks::TokioTaskExecutor;

use super::{pool::Pool, pre_proposal_agg::PreProposalAggregationBuilder};
use crate::{mocks::validator::MockValidator, type_generator::amm::AMMSnapshotBuilder};

#[derive(Debug, Default)]
pub struct ProposalBuilder {
    ethereum_height:   Option<u64>,
    order_count:       Option<usize>,
    preproposals:      Option<Vec<PreProposalAggregation>>,
    preproposal_count: Option<usize>,
    block:             Option<u64>,
    pools:             Option<Vec<Pool>>,
    sk:                Option<AngstromSigner>
}

impl ProposalBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn order_count(self, order_count: usize) -> Self {
        Self { order_count: Some(order_count), ..self }
    }

    pub fn preproposals(self, preproposals: Vec<PreProposalAggregation>) -> Self {
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

    pub fn with_secret_key(self, sk: AngstromSigner) -> Self {
        Self { sk: Some(sk), ..self }
    }

    pub fn build(self) -> Proposal {
        // Extract values from our struct
        let ethereum_height = self.ethereum_height.unwrap_or_default();
        let preproposal_count = self.preproposal_count.unwrap_or_default();
        let pools = self.pools.unwrap_or_default();
        let count = self.order_count.unwrap_or_default();
        let block = self.block.unwrap_or_default();
        let sk = self.sk.unwrap_or_else(AngstromSigner::random);
        // Build the source ID from the secret/public keypair

        let preproposals = self.preproposals.unwrap_or_else(|| {
            (0..preproposal_count)
                .map(|_| {
                    PreProposalAggregationBuilder::new()
                        .for_block(block)
                        .order_count(count)
                        .for_pools(pools.clone())
                        .with_secret_key(sk.clone())
                        .build()
                })
                .collect::<Vec<_>>()
        });

        let books = MatchingManager::<TokioTaskExecutor, MockValidator>::build_books(
            &preproposals[0].pre_proposals,
            &HashMap::default()
        );
        let searcher_orders: HashMap<PoolId, OrderWithStorageData<TopOfBlockOrder>> = preproposals
            .iter()
            .flat_map(|p| p.pre_proposals.iter())
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
        Proposal::generate_proposal(ethereum_height, &sk, preproposals, solutions)
    }
}
