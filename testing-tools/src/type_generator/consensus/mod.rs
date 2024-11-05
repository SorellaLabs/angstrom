pub mod pool;
pub mod preproposal;
pub mod proposal;

use angstrom_types::sol_bindings::grouped_orders::{GroupedVanillaOrder, OrderWithStorageData};

use super::orders::UserOrderBuilder;

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
