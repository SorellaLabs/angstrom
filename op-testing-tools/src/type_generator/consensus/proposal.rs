use alloy::signers::local::PrivateKeySigner;
use alloy_primitives::{
    Address,
    aliases::{I24, U24}
};
use angstrom_types::{
    consensus::PreProposalAggregation,
    contract_bindings::angstrom::Angstrom::PoolKey,
    matching::{SqrtPriceX96, uniswap::LiqRange},
    primitive::AngstromSigner
};

use super::pool::Pool;
use crate::type_generator::amm::AMMSnapshotBuilder;

#[derive(Debug, Default)]
pub struct ProposalBuilder {
    pub ethereum_height:   Option<u64>,
    pub order_count:       Option<usize>,
    pub preproposals:      Option<Vec<PreProposalAggregation>>,
    pub preproposal_count: Option<usize>,
    pub block:             Option<u64>,
    pub pools:             Option<Vec<Pool>>,
    pub sk:                Option<AngstromSigner<PrivateKeySigner>>
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
                        LiqRange::new_init(99000, 101000, 1_000_000_000_000_000_u128, 0, true)
                            .unwrap(),
                        LiqRange::new_init(99000, 101000, 1_000_000_000_000_000_u128, 0, false)
                            .unwrap(),
                    ])
                    .build();
                Pool::new(key, amm, Address::random())
            })
            .collect();
        Self { pools: Some(pools), ..self }
    }

    pub fn with_secret_key(self, sk: AngstromSigner<PrivateKeySigner>) -> Self {
        Self { sk: Some(sk), ..self }
    }
}
