use alloy::signers::local::PrivateKeySigner;
use angstrom_types::{
    consensus::PreProposal, primitive::AngstromSigner, sol_bindings::RawPoolOrder
};

use super::pool::{Pool, PoolBuilder};
use crate::type_generator::orders::{
    DistributionParameters, OrderDistributionBuilder, ToBOrderBuilder
};

#[derive(Debug, Default)]
pub struct PreproposalBuilder {
    order_count: Option<usize>,
    block:       Option<u64>,
    pools:       Option<Vec<Pool>>,
    sk:          Option<AngstromSigner<PrivateKeySigner>>
}

impl PreproposalBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn order_count(self, order_count: usize) -> Self {
        Self { order_count: Some(order_count), ..self }
    }

    pub fn for_block(self, block: u64) -> Self {
        Self { block: Some(block), ..self }
    }

    pub fn for_pools(self, pools: Vec<Pool>) -> Self {
        Self { pools: Some(pools), ..self }
    }

    pub fn for_random_pools(self, pool_count: usize) -> Self {
        let pools: Vec<Pool> = (0..pool_count)
            .map(|_| PoolBuilder::new().build())
            .collect();
        Self { pools: Some(pools), ..self }
    }

    pub fn with_secret_key(self, sk: AngstromSigner<PrivateKeySigner>) -> Self {
        Self { sk: Some(sk), ..self }
    }

    pub fn build(self) -> PreProposal {
        // Extract values from our struct
        let pools = self.pools.unwrap_or_default();
        // let pools: Vec<PoolId> = self
        //     .pools
        //     .map(|p| p.iter().map(|key| key.id()).collect())
        //     .unwrap_or_default();
        let count = self.order_count.unwrap_or_default();
        let block = self.block.unwrap_or_default();
        let sk = self.sk.unwrap_or_else(AngstromSigner::random);
        // Build the source ID from the secret/public keypair

        let limit = pools
            .iter()
            .flat_map(|pool| {
                let (bid_dist, ask_dist) =
                    DistributionParameters::crossed_at(pool.price().as_float());
                let (bid_quant, ask_quant) = DistributionParameters::fixed_at(100.0);
                let bids = OrderDistributionBuilder::new()
                    .bid()
                    .order_count(count)
                    .pool_id(pool.id())
                    .valid_block(block)
                    .price_params(bid_dist)
                    .volume_params(bid_quant)
                    .signing_key(Some(sk.clone()))
                    .build_hashes()
                    .unwrap();
                let asks = OrderDistributionBuilder::new()
                    .ask()
                    .order_count(count)
                    .pool_id(pool.id())
                    .valid_block(block)
                    .price_params(ask_dist)
                    .volume_params(ask_quant)
                    .signing_key(Some(sk.clone()))
                    .build_hashes()
                    .unwrap();
                [bids, asks].concat()
            })
            .collect();

        let searcher = pools
            .iter()
            .map(|pool_id| {
                ToBOrderBuilder::new()
                    .recipient(pool_id.tob_recipient())
                    .asset_in(pool_id.token1())
                    .asset_out(pool_id.token0())
                    .quantity_in(2_201_872_310_000_u128)
                    .quantity_out(100000000_u128)
                    .signing_key(Some(sk.clone()))
                    .valid_block(block)
                    .build()
                    .order_hash()
            })
            .collect();

        PreProposal::generate_pre_proposal(block, &sk, limit, searcher)
    }
}
