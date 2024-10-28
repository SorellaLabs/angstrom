pub mod preproposal;
pub mod proposal;

use angstrom_types::{
    primitive::PoolId,
    sol_bindings::grouped_orders::{GroupedVanillaOrder, OrderWithStorageData}
};

use super::orders::{DistributionParameters, UserOrderBuilder};
use crate::type_generator::orders::generate_order_distribution;

pub fn generate_limit_order_set(
    count: usize,
    is_bid: bool,
    block: u64
) -> Vec<OrderWithStorageData<GroupedVanillaOrder>> {
    (0..count)
        .map(|_| {
            UserOrderBuilder::new()
                .kill_or_fill()
                .block(block)
                .with_storage()
                .valid_block(block)
                .is_bid(is_bid)
                .build()
        })
        .collect()
}

pub fn generate_limit_order_distribution(
    count: usize,
    pool_id: PoolId,
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

#[cfg(test)]
mod tests {
    use crate::type_generator::consensus::{
        preproposal::PreproposalBuilder, proposal::ProposalBuilder
    };

    #[test]
    fn random_preproposal_is_valid() {
        let preproposal = PreproposalBuilder::new()
            .order_count(100)
            .for_random_pools(1)
            .for_block(10)
            .build();
        assert!(preproposal.is_valid(), "Preproposal cannot validate itself");
    }

    #[test]
    fn random_proposal_is_valid() {
        let proposal = ProposalBuilder::new()
            .order_count(100)
            .for_random_pools(1)
            .for_block(10)
            .build();
        assert!(proposal.is_valid(), "Proposal cannot validate itself");
    }
}
