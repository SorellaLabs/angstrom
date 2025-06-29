use alloy::primitives::{I256, U160};
use angstrom_types::{
    matching::SqrtPriceX96,
    orders::PoolSolution,
    sol_bindings::{grouped_orders::OrderWithStorageData, rpc_orders::TopOfBlockOrder}
};

use crate::{book::OrderBook, matcher::delta::DeltaMatcher};

pub struct BinarySearchStrategy {}

impl BinarySearchStrategy {
    pub fn run(
        book: &OrderBook,
        searcher: Option<OrderWithStorageData<TopOfBlockOrder>>
    ) -> PoolSolution {
        let mut matcher = DeltaMatcher::new(book, searcher.clone().into(), false);
        matcher.solution(searcher)
    }

    pub fn give_end_amm_state(
        book: &OrderBook,
        searcher: Option<OrderWithStorageData<TopOfBlockOrder>>
    ) -> (U160, i32) {
        let snapshot = book.amm().unwrap();

        if book.is_empty_book() {
            return (*snapshot.current_price(), snapshot.current_tick());
        }

        let mut matcher = DeltaMatcher::new(book, searcher.clone().into(), false);
        let solution = matcher.solution(searcher);

        // we have no book currently attached
        if solution.ucp.is_zero() {
            let amm = matcher.try_get_amm_location();
            (*amm.end_price, amm.end_tick)
        } else {
            // same flow as bundle building
            let post_tob_swap = matcher.try_get_amm_location();

            let ucp: SqrtPriceX96 = solution.ucp.into();
            // grab amount in when swap to price, then from there, calculate
            // actual values.
            let book_swap_vec = post_tob_swap.swap_to_price(ucp);

            // if zero for 1 is neg
            let net_t0 = book_swap_vec
                .as_ref()
                .map(|b| b.t0_signed())
                .unwrap_or(I256::ZERO)
                + post_tob_swap.t0_signed();

            let net_direction = net_t0.is_negative();

            let amount_in = if net_t0.is_negative() {
                net_t0.unsigned_abs()
            } else {
                (book_swap_vec
                    .as_ref()
                    .map(|b| b.t1_signed())
                    .unwrap_or(I256::ZERO)
                    + post_tob_swap.t1_signed())
                .unsigned_abs()
            };

            let res = snapshot
                .swap_current_with_amount(I256::from_raw(amount_in), net_direction)
                .unwrap()
                .clone();

            (*res.end_price, res.end_tick)
        }
    }
}
