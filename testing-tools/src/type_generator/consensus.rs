use std::collections::HashMap;

use angstrom_types::{
    consensus::{Commit, PreProposal, Proposal},
    sol_bindings::{
        grouped_orders::{GroupedVanillaOrder, OrderWithStorageData},
        sol::TopOfBlockOrder
    }
};
use blsful::{Bls12381G1Impl, SecretKey};
use matching_engine::{
    strategy::{MatchingStrategy, SimpleCheckpointStrategy},
    MatchingManager
};
use rand::{rngs::ThreadRng, thread_rng, Rng};
use reth_network_peers::pk2id;
use secp256k1::{Secp256k1, SecretKey as Secp256SecretKey};

use super::orders::{generate_top_of_block_order, DistributionParameters};
use crate::type_generator::orders::{generate_limit_order, generate_order_distribution};

pub fn generate_limit_order_set(
    rng: &mut ThreadRng,
    count: usize,
    is_bid: bool,
    block: u64
) -> Vec<OrderWithStorageData<GroupedVanillaOrder>> {
    (0..count)
        .map(|_| generate_limit_order(rng, true, is_bid, None, Some(block), None, None,None))
        .collect()
}

pub fn generate_limit_order_distribution(
    count: usize,
    pool_id: usize,
    block: u64
) -> Vec<OrderWithStorageData<GroupedVanillaOrder>> {
    let mut res = Vec::with_capacity(count * 2);
    let (bidprice, askprice) = DistributionParameters::crossed_at(100_000_000.0);
    let (bidquant, askquant) = DistributionParameters::fixed_at(100.0);
    res.extend(
        generate_order_distribution(true, count, bidprice, bidquant, pool_id, block).unwrap()
    );
    res.extend(
        generate_order_distribution(false, count, askprice, askquant, pool_id, block).unwrap()
    );
    res
}

pub fn generate_random_preproposal(count: usize, pool_count: usize, block: u64) -> PreProposal {
    let mut rng = thread_rng();
    let sk = Secp256SecretKey::new(&mut rng);
    let secp = Secp256k1::new();
    let pk = sk.public_key(&secp);
    // Grab the source ID from the secret/public keypair
    let source = pk2id(&pk);
    let limit = (0..pool_count)
        .flat_map(|pool_id| generate_limit_order_distribution(count, pool_id, block))
        .collect();

    let searcher = (0..pool_count)
        .map(|pool_id| generate_top_of_block_order(&mut rng, true, Some(pool_id), Some(block)))
        .collect();

    PreProposal::generate_pre_proposal(block, source, limit, searcher, &sk)
}

pub fn generate_random_proposal(count: usize, pool_count: usize, block: u64) -> Proposal {
    let mut rng = thread_rng();
    let sk = Secp256SecretKey::new(&mut rng);
    let secp = Secp256k1::new();
    let pk = sk.public_key(&secp);
    // Grab the source ID from the secret/public keypair
    let source = pk2id(&pk);

    let preproposals = (0..5)
        .map(|_| generate_random_preproposal(count, pool_count, block))
        .collect::<Vec<_>>();
    let manager = MatchingManager {};
    let books = manager.build_books(&preproposals);
    let searcher_orders: HashMap<usize, OrderWithStorageData<TopOfBlockOrder>> = preproposals
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

    Proposal::generate_proposal(rng.gen(), source, preproposals, solutions, &sk)
}

pub fn generate_random_commit(sk: &SecretKey<Bls12381G1Impl>) -> Commit {
    let proposal = generate_random_proposal(100, 10, 10);
    Commit::from_proposal(&proposal, sk)
}

#[cfg(test)]
mod tests {
    use rand::thread_rng;

    use super::{generate_random_commit, generate_random_preproposal, generate_random_proposal};

    #[test]
    fn random_preproposal_is_valid() {
        let preproposal = generate_random_preproposal(100, 1, 10);
        assert!(preproposal.validate(), "Preproposal cannot validate itself");
    }

    #[test]
    fn random_proposal_is_valid() {
        let proposal = generate_random_proposal(100, 1, 10);
        assert!(proposal.validate(), "Proposal cannot validate itself");
    }

    #[test]
    fn random_commit_is_valid() {
        // We generate our secret key first
        let sk = blsful::SecretKey::random(&mut thread_rng());
        let commit = generate_random_commit(&sk);
        assert!(commit.validate(&[sk.public_key()]));
    }
}
