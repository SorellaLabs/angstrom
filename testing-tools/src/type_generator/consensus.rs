use alloy_primitives::Bytes;
use angstrom_types::{
    consensus::{Commit, OrderBuffer, PoolOrders, PreProposal, Proposal},
    primitive::{BLSValidatorID, Bundle, LowerBound, Lvr}
};
use blsful::SecretKey;
use rand::{rngs::ThreadRng, thread_rng, Rng};
use secp256k1::SecretKey as Secp256SecretKey;

use crate::type_generator::orders::generate_rand_valid_limit_order;

pub fn generate_random_commit() -> Commit {
    let mut rng = thread_rng();
    let sk = SecretKey::new();

    Commit::generate_commit_all(rng.gen(), rng.gen(), rng.gen(), rng.gen(), &sk)
}

pub fn generate_random_preposal() -> PreProposal {
    let mut rng = thread_rng();
    let sk = Secp256SecretKey::new(&mut rng);

    let mut pre_bundle = Vec::new();
    for _ in 0..rng.gen_range(3..10) {
        pre_bundle.push(PoolOrders {
            pool:         angstrom_types::primitive::PoolKey {
                currency0:   rng.gen(),
                currency1:   rng.gen(),
                fee:         rng.gen(),
                tickSpacing: rng.gen(),
                hooks:       rng.gen()
            },
            sorted_bids:  (0..rng.gen_range(3..10))
                .map(|_| generate_rand_valid_limit_order())
                .collect(),
            sorted_asks:  (0..rng.gen_range(3..10))
                .map(|_| generate_rand_valid_limit_order())
                .collect(),
            searcher_bid: generate_rand_valid_limit_order()
        })
    }

    PreProposal::generate_pre_proposal(rng.gen(), pre_bundle, &sk)
}

pub fn generate_random_proposal(validator_id: BLSValidatorID) -> Proposal {
    let mut rng = thread_rng();
    let sk = SecretKey::new();

    let mut order_buf = Vec::new();
    for _ in 0..rng.gen_range(5..10) {
        order_buf.push(OrderBuffer {
            excess_orders:  (0..rng.gen_range(3..10))
                .map(|_| generate_rand_valid_limit_order())
                .collect(),
            reserve_orders: (0..rng.gen_range(3..10))
                .map(|_| generate_rand_valid_limit_order())
                .collect()
        })
    }
    let lower_bound = generate_lower_bound(&mut rng);
    let vanilla_bundle = generate_vanilla_bundle(&mut rng);

    Proposal::generate_proposal(
        rng.gen(),
        vanilla_bundle,
        lower_bound,
        order_buf,
        validator_id,
        &sk
    )
}

fn generate_vanilla_bundle(_rng: &mut ThreadRng) -> Bundle {
    Bundle { orders: vec![], composableOrders: vec![], uniswapData: Bytes::new() }
}

fn generate_lower_bound(rng: &mut ThreadRng) -> LowerBound {
    LowerBound {
        proposer: rng.gen(),
        lvr_comp: (0..rng.gen_range(3..10)).map(|_| gen_lvr(rng)).collect()
    }
}

fn gen_lvr(rng: &mut ThreadRng) -> Lvr {
    Lvr { postArbPrice: rng.gen(), lvrComp: rng.gen(), proportion: rng.gen() }
}
