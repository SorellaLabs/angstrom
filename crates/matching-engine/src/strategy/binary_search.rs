use angstrom_types::{
    orders::PoolSolution,
    sol_bindings::{grouped_orders::OrderWithStorageData, rpc_orders::TopOfBlockOrder}
};

use crate::{
    book::OrderBook,
    matcher::{binary_search::BinarySearchMatcher, delta::DeltaMatcher}
};

pub struct BinarySearchStrategy {}

impl BinarySearchStrategy {
    pub fn run(
        book: &OrderBook,
        searcher: Option<OrderWithStorageData<TopOfBlockOrder>>
    ) -> PoolSolution {
        let mut matcher = DeltaMatcher::new(book, searcher.clone());
        matcher.solution(searcher)
    }
}
