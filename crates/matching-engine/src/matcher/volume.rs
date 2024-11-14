use std::{cell::Cell, cmp::Ordering};

use alloy::primitives::U256;
use angstrom_types::{
    matching::{match_estimate_response::PoolEstimate, uniswap::PoolPrice, Ray, SqrtPriceX96},
    orders::{NetAmmOrder, OrderFillState, OrderOutcome, PoolSolution},
    sol_bindings::{
        grouped_orders::{GroupedVanillaOrder, OrderWithStorageData},
        rpc_orders::TopOfBlockOrder
    }
};

use super::Solution;
use crate::book::{
    order::{OrderContainer, OrderExclusion},
    OrderBook
};

type CrossPoolExclusions = Option<(Vec<Option<OrderExclusion>>, Vec<Option<OrderExclusion>>)>;

pub enum VolumeFillMatchEndReason {
    NoMoreBids,
    NoMoreAsks,
    BothSidesAMM,
    NoLongerCross,
    ZeroQuantity
}

#[derive(Clone)]
pub struct VolumeFillMatcher<'a> {
    book:             &'a OrderBook,
    bid_idx:          Cell<usize>,
    pub bid_outcomes: Vec<OrderFillState>,
    bid_xpool:        Vec<Option<OrderExclusion>>,
    ask_idx:          Cell<usize>,
    pub ask_outcomes: Vec<OrderFillState>,
    ask_xpool:        Vec<Option<OrderExclusion>>,
    amm_price:        Option<PoolPrice<'a>>,
    amm_outcome:      Option<NetAmmOrder>,
    current_partial:  Option<OrderWithStorageData<GroupedVanillaOrder>>,
    results:          Solution,
    // A checkpoint should never have a checkpoint stored within itself, otherwise this gets gnarly
    checkpoint:       Option<Box<Self>>
}

impl<'a> VolumeFillMatcher<'a> {
    pub fn new(book: &'a OrderBook) -> Self {
        let mut new_element = Self::with_related(book, None);
        // We can checkpoint our initial state as valid
        new_element.save_checkpoint();
        new_element
    }

    fn with_related(book: &'a OrderBook, xpool: CrossPoolExclusions) -> Self {
        let (bid_xpool, ask_xpool) =
            xpool.unwrap_or_else(|| (vec![None; book.bids().len()], vec![None; book.asks().len()]));
        let bid_outcomes = vec![OrderFillState::Unfilled; book.bids().len()];
        let ask_outcomes = vec![OrderFillState::Unfilled; book.asks().len()];
        let amm_price = book.amm().map(|a| a.current_price());
        Self {
            book,
            bid_idx: Cell::new(0),
            bid_outcomes,
            bid_xpool,
            ask_idx: Cell::new(0),
            ask_outcomes,
            ask_xpool,
            amm_price,
            amm_outcome: None,
            current_partial: None,
            results: Solution::default(),
            checkpoint: None
        }
    }

    pub fn results(&self) -> &Solution {
        &self.results
    }

    /// Save our current solve state to an internal checkpoint
    fn save_checkpoint(&mut self) {
        let checkpoint = Self {
            book:            self.book,
            bid_idx:         self.bid_idx.clone(),
            bid_outcomes:    self.bid_outcomes.clone(),
            bid_xpool:       self.bid_xpool.clone(),
            ask_idx:         self.ask_idx.clone(),
            ask_outcomes:    self.ask_outcomes.clone(),
            ask_xpool:       self.ask_xpool.clone(),
            amm_price:       self.amm_price.clone(),
            amm_outcome:     self.amm_outcome.clone(),
            current_partial: self.current_partial.clone(),
            results:         self.results.clone(),
            checkpoint:      None
        };
        self.checkpoint = Some(Box::new(checkpoint));
    }

    /// Spawn a new VolumeFillBookSolver from our checkpoint
    pub fn from_checkpoint(&self) -> Option<Self> {
        self.checkpoint.as_ref().map(|cp| *cp.clone())
    }

    /// Restore our checkpoint into this VolumeFillBookSolver - not sure if we
    /// ever want to do this but we can!
    #[allow(dead_code)]
    fn restore_checkpoint(&mut self) -> bool {
        let Some(checkpoint) = self.checkpoint.take() else {
            return false;
        };
        let Self {
            bid_idx, bid_outcomes, ask_idx, ask_outcomes, amm_price, current_partial, ..
        } = *checkpoint;
        self.bid_idx = bid_idx;
        self.bid_outcomes = bid_outcomes;
        self.ask_idx = ask_idx;
        self.ask_outcomes = ask_outcomes;
        self.amm_price = amm_price;
        self.current_partial = current_partial;
        true
    }

    pub fn fill(&mut self) -> VolumeFillMatchEndReason {
        {
            loop {
                let bid = match self.current_partial {
                    Some(ref o) if o.is_bid => OrderContainer::BookOrderFragment(o),
                    _ => {
                        if let Some(o) = Self::next_order_from_book(
                            true,
                            &self.bid_idx,
                            self.book.bids(),
                            &self.bid_outcomes,
                            self.amm_price.as_ref()
                        ) {
                            o
                        } else {
                            return VolumeFillMatchEndReason::NoMoreBids
                        }
                    }
                };
                let ask = match self.current_partial {
                    Some(ref o) if !o.is_bid => OrderContainer::BookOrderFragment(o),
                    _ => {
                        if let Some(o) = Self::next_order_from_book(
                            false,
                            &self.ask_idx,
                            self.book.asks(),
                            &self.ask_outcomes,
                            self.amm_price.as_ref()
                        ) {
                            o
                        } else {
                            return VolumeFillMatchEndReason::NoMoreBids
                        }
                    }
                };

                // If we're talking to the AMM on both sides, we're done
                if bid.is_amm() && ask.is_amm() {
                    return VolumeFillMatchEndReason::BothSidesAMM
                }

                // If our prices no longer cross, we're done
                if ask.price() > bid.price() {
                    return VolumeFillMatchEndReason::NoLongerCross
                }

                // Limit to price so that AMM orders will only offer the quantity they can
                // profitably sell.  (Non-AMM orders ignore the provided price)
                let ask_q = ask.quantity(bid.price());
                let bid_q = bid.quantity(ask.price());

                // If either quantity is zero maybe we should break here? (could be a
                // replacement for price cross checking if we implement that)
                if ask_q == U256::ZERO || bid_q == U256::ZERO {
                    return VolumeFillMatchEndReason::ZeroQuantity
                }

                let matched = ask_q.min(bid_q);
                // Store the amount we matched
                self.results.total_volume += matched;

                // Record partial fills
                if bid.is_partial() {
                    self.results.partial_volume.0 += matched;
                }
                if ask.is_partial() {
                    self.results.partial_volume.1 += matched;
                }

                // If bid or ask was an AMM order, we update our AMM stats
                if let (OrderContainer::AMM(o), _) | (_, OrderContainer::AMM(o)) = (&bid, &ask) {
                    // We always update our AMM price with any quantity sold
                    let final_amm_order = o.fill(matched);
                    self.amm_price = Some(final_amm_order.end_bound.clone());
                    // Add to our solution
                    self.results.amm_volume += matched;
                    self.results.amm_final_price = Some(*final_amm_order.end_bound.price());
                    // Update our overall AMM volume
                    let amm_out = self
                        .amm_outcome
                        .get_or_insert_with(|| NetAmmOrder::new(bid.is_amm()));
                    amm_out.add_quantity(final_amm_order.d_t0, final_amm_order.d_t1);
                }

                // Then we see what else we need to do
                match bid_q.cmp(&ask_q) {
                    Ordering::Equal => {
                        // We annihilated
                        self.results.price =
                            Some((*(ask.price() + bid.price()) / U256::from(2)).into());
                        // self.results.price = Some((ask.price() + bid.price()) / 2.0_f64);
                        // Mark as filled if non-AMM order
                        if !ask.is_amm() {
                            self.ask_outcomes[self.ask_idx.get()] = OrderFillState::CompleteFill
                        }
                        if !bid.is_amm() {
                            self.bid_outcomes[self.bid_idx.get()] = OrderFillState::CompleteFill
                        }
                        self.current_partial = None;
                        // Take a snapshot as a good solve state
                        self.save_checkpoint();
                        // We're done here, we'll get our next bid and ask on
                        // the next round
                    }
                    Ordering::Greater => {
                        self.results.price = Some(bid.price());
                        // Ask was completely filled, remainder bid
                        if !ask.is_amm() {
                            self.ask_outcomes[self.ask_idx.get()] = OrderFillState::CompleteFill
                        }
                        // Create and save our partial bid
                        if !bid.is_amm() {
                            self.bid_outcomes[self.bid_idx.get()] =
                                self.bid_outcomes[self.bid_idx.get()].partial_fill(matched);
                            self.current_partial = Some(bid.fill(ask_q));
                        } else {
                            self.current_partial = None;
                        }
                    }
                    Ordering::Less => {
                        self.results.price = Some(ask.price());
                        // Bid was completely filled, remainder ask
                        if !bid.is_amm() {
                            self.bid_outcomes[self.bid_idx.get()] = OrderFillState::CompleteFill
                        }
                        // Create and save our parital ask
                        if !ask.is_amm() {
                            self.ask_outcomes[self.ask_idx.get()] =
                                self.ask_outcomes[self.ask_idx.get()].partial_fill(matched);
                            self.current_partial = Some(ask.fill(bid_q));
                        } else {
                            self.current_partial = None;
                        }
                    }
                }
                // We can checkpoint if we annihilated (No partial), if we completely filled an
                // order with an AMM order (No partial) or if we have an incomplete order but
                // it's a Partial Fill order which means this is a valid state to stop
                if let Some(ref fragment) = self.current_partial {
                    if fragment.is_partial() {
                        self.save_checkpoint();
                    }
                }
            }
        }
    }

    fn next_order_from_book<'b>(
        is_bid: bool,
        index: &Cell<usize>,
        book: &'a [OrderWithStorageData<GroupedVanillaOrder>],
        fill_state: &[OrderFillState],
        amm: Option<&PoolPrice<'a>>
    ) -> Option<OrderContainer<'a, 'b>> {
        let mut cur_idx = index.get();
        // Find the next unfilled order - we need to work with the index separately
        while cur_idx < fill_state.len() {
            match &fill_state[cur_idx] {
                OrderFillState::Unfilled => break,
                _ => cur_idx += 1
            }
        }
        let book_order = book.get(cur_idx);
        // See if our AMM takes precedence
        amm.and_then(|amm_price| {
            let target_price = book_order
                .map(|o| SqrtPriceX96::from(Ray::from(*OrderContainer::BookOrder(o).price())));
            amm_price.order_to_target(target_price, !is_bid)
        })
        .map(OrderContainer::AMM)
        .or_else(|| {
            index.set(cur_idx);
            book_order.map(OrderContainer::BookOrder)
        })
    }

    pub fn estimate(
        &self,
        searcher: Option<OrderWithStorageData<TopOfBlockOrder>>
    ) -> PoolEstimate {
        let limit = self
            .bid_outcomes
            .iter()
            .enumerate()
            .map(|(idx, outcome)| (self.book.bids()[idx].order_id, outcome))
            .chain(
                self.ask_outcomes
                    .iter()
                    .enumerate()
                    .map(|(idx, outcome)| (self.book.asks()[idx].order_id, outcome))
            )
            .map(|(id, outcome)| OrderOutcome { id, outcome: outcome.clone() })
            .collect();
        PoolEstimate {
            orders: self.bid_outcomes.len() as u64,
            gas_in_wei
        }

        todo!()
    }

    pub fn solution(
        &self,
        searcher: Option<OrderWithStorageData<TopOfBlockOrder>>
    ) -> PoolSolution {
        let limit = self
            .bid_outcomes
            .iter()
            .enumerate()
            .map(|(idx, outcome)| (self.book.bids()[idx].order_id, outcome))
            .chain(
                self.ask_outcomes
                    .iter()
                    .enumerate()
                    .map(|(idx, outcome)| (self.book.asks()[idx].order_id, outcome))
            )
            .map(|(id, outcome)| OrderOutcome { id, outcome: outcome.clone() })
            .collect();
        let ucp: Ray = self.results.price.map(Into::into).unwrap_or_default();
        PoolSolution {
            id: self.book.id(),
            ucp,
            amm_quantity: self.amm_outcome.clone(),
            searcher,
            limit
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use alloy::primitives::Uint;
    use angstrom_types::{
        matching::Ray,
        orders::OrderFillState,
        primitive::PoolId,
        sol_bindings::grouped_orders::{GroupedVanillaOrder, OrderWithStorageData}
    };
    use testing_tools::type_generator::orders::UserOrderBuilder;

    use super::VolumeFillMatcher;
    use crate::book::OrderBook;

    #[test]
    fn runs_cleanly_on_empty_book() {
        let book = OrderBook::default();
        let matcher = VolumeFillMatcher::new(&book);
        let solution = matcher.solution(None);
        assert!(solution.ucp == Ray::ZERO, "Empty book didn't have UCP of zero");
    }

    // Let's write tests for all the basic matching outcomes to make sure they
    // work properly, then come up with some more complicated situations and
    // components to check

    #[test]
    fn bid_outweighs_ask_sets_price() {
        let pool_id = PoolId::random();
        let high_price = Ray::from(Uint::from(1_000_000_000_u128));
        let low_price = Ray::from(Uint::from(1_000_u128));
        let bid_order = UserOrderBuilder::new()
            .partial()
            .amount(100)
            .min_price(high_price)
            .with_storage()
            .bid()
            .build();
        let ask_order = UserOrderBuilder::new()
            .exact()
            .amount(10)
            .min_price(low_price)
            .with_storage()
            .ask()
            .build();
        let book = OrderBook::new(pool_id, None, vec![bid_order.clone()], vec![ask_order], None);
        let mut matcher = VolumeFillMatcher::new(&book);
        let _fill_outcome = matcher.fill();
        let solution = matcher.from_checkpoint().unwrap().solution(None);
        assert!(
            solution.ucp == high_price,
            "Bid outweighed but the final price wasn't properly set"
        );
    }

    #[test]
    fn ask_outweighs_bid_sets_price() {
        let pool_id = PoolId::random();
        let high_price = Ray::from(Uint::from(1_000_000_000_u128));
        let low_price = Ray::from(Uint::from(1_000_u128));
        let bid_order = UserOrderBuilder::new()
            .exact()
            .amount(10)
            .min_price(high_price)
            .with_storage()
            .bid()
            .build();
        let ask_order = UserOrderBuilder::new()
            .partial()
            .amount(100)
            .min_price(low_price)
            .with_storage()
            .ask()
            .build();
        let book = OrderBook::new(pool_id, None, vec![bid_order.clone()], vec![ask_order], None);
        let mut matcher = VolumeFillMatcher::new(&book);
        let _fill_outcome = matcher.fill();
        let solution = matcher.from_checkpoint().unwrap().solution(None);
        assert!(
            solution.ucp == low_price,
            "Ask outweighed but the final price wasn't properly set"
        );
    }

    fn basic_order_book(
        is_bid: bool,
        count: usize,
        target_price: Ray,
        price_step: usize
    ) -> Vec<OrderWithStorageData<GroupedVanillaOrder>> {
        (0..count)
            .map(|i| {
                UserOrderBuilder::new()
                    .min_price(target_price + (i * price_step))
                    .amount(100)
                    .with_storage()
                    .is_bid(is_bid)
                    .build()
            })
            .collect()
    }

    #[test]
    fn gets_next_book_order() {
        let is_bid = true;
        let index = Cell::new(10);
        let book = basic_order_book(true, 100, Ray::from(10000_usize), 10);
        let fill_state: Vec<OrderFillState> =
            book.iter().map(|_| OrderFillState::Unfilled).collect();
        let amm = None;
        let next_order =
            VolumeFillMatcher::next_order_from_book(is_bid, &index, &book, &fill_state, amm);
        assert!(next_order.is_none())
    }
}
