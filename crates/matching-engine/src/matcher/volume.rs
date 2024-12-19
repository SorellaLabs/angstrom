use std::{
    cell::Cell,
    cmp::{max, Ordering}
};

use alloy::primitives::U256;
use angstrom_types::{
    matching::{
        uniswap::{Direction, PoolPrice, PoolPriceVec},
        CompositeOrder, Debt, Ray
    },
    orders::{NetAmmOrder, OrderFillState, OrderOutcome, PoolSolution},
    sol_bindings::{grouped_orders::OrderWithStorageData, rpc_orders::TopOfBlockOrder}
};

use super::Solution;
use crate::book::{order::OrderContainer, BookOrder, OrderBook};

#[derive(Debug)]
pub enum VolumeFillMatchEndReason {
    NoMoreBids,
    NoMoreAsks,
    BothSidesAMM,
    NoLongerCross,
    ZeroQuantity,
    /// This SHOULDN'T happen but I'm using it to clean up problem spots in the
    /// code
    ErrorEncountered
}

#[derive(Clone)]
pub struct VolumeFillMatcher<'a> {
    book:             &'a OrderBook,
    bid_idx:          Cell<usize>,
    pub bid_outcomes: Vec<OrderFillState>,
    ask_idx:          Cell<usize>,
    pub ask_outcomes: Vec<OrderFillState>,
    debt:             Option<Debt>,
    amm_price:        Option<PoolPrice<'a>>,
    amm_outcome:      Option<NetAmmOrder>,
    results:          Solution,
    // A checkpoint should never have a checkpoint stored within itself, otherwise this gets gnarly
    checkpoint:       Option<Box<Self>>
}

impl<'a> VolumeFillMatcher<'a> {
    pub fn new(book: &'a OrderBook) -> Self {
        let bid_cnt = book.bids().len();
        let ask_cnt = book.asks().len();
        tracing::info!(?bid_cnt, ?ask_cnt);
        let bid_outcomes = vec![OrderFillState::Unfilled; book.bids().len()];
        let ask_outcomes = vec![OrderFillState::Unfilled; book.asks().len()];
        let amm_price = book.amm().map(|a| a.current_price());
        let mut new_element = Self {
            book,
            bid_idx: Cell::new(0),
            bid_outcomes,
            ask_idx: Cell::new(0),
            ask_outcomes,
            debt: None,
            amm_price,
            amm_outcome: None,
            results: Solution::default(),
            checkpoint: None
        };
        // We can checkpoint our initial state as valid
        new_element.save_checkpoint();
        new_element
    }

    pub fn results(&self) -> &Solution {
        &self.results
    }

    /// Save our current solve state to an internal checkpoint
    fn save_checkpoint(&mut self) {
        let checkpoint = Self {
            book:         self.book,
            bid_idx:      self.bid_idx.clone(),
            bid_outcomes: self.bid_outcomes.clone(),
            ask_idx:      self.ask_idx.clone(),
            ask_outcomes: self.ask_outcomes.clone(),
            debt:         self.debt,
            amm_price:    self.amm_price.clone(),
            amm_outcome:  self.amm_outcome.clone(),
            results:      self.results.clone(),
            checkpoint:   None
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
        let Self { bid_idx, bid_outcomes, ask_idx, ask_outcomes, amm_price, .. } = *checkpoint;
        self.bid_idx = bid_idx;
        self.bid_outcomes = bid_outcomes;
        self.ask_idx = ask_idx;
        self.ask_outcomes = ask_outcomes;
        self.amm_price = amm_price;
        true
    }

    fn fill_amm(
        amm: &mut PoolPrice<'a>,
        results: &mut Solution,
        amm_outcome: &mut Option<NetAmmOrder>,
        quantity: u128,
        direction: Direction
    ) -> eyre::Result<()> {
        let new_amm = amm.d_t0(quantity, direction)?;
        let final_amm_order = PoolPriceVec::from_price_range(amm.clone(), new_amm.clone())?;
        *amm = new_amm.clone();
        // Add to our solution
        results.amm_volume += quantity;
        results.amm_final_price = Some(*new_amm.price());
        // Update our overall AMM volume
        let is_bid = matches!(direction, Direction::BuyingT0);
        let amm_out = amm_outcome.get_or_insert_with(|| NetAmmOrder::new(is_bid));
        amm_out.add_quantity(U256::from(final_amm_order.d_t0), U256::from(final_amm_order.d_t1));
        Ok(())
    }

    pub fn run_match(&mut self) -> VolumeFillMatchEndReason {
        // Run our match over and over until we get an end reason
        loop {
            if let Some(r) = self.single_match() {
                tracing::debug!(?r);
                return r
            }
        }
    }

    pub fn single_match(&mut self) -> Option<VolumeFillMatchEndReason> {
        tracing::info!("single match");
        // Get the bid order
        let Some(bid) = Self::next_order(
            true,
            &self.bid_idx,
            &self.debt,
            self.amm_price.as_ref(),
            self.book.bids(),
            &self.bid_outcomes
        ) else {
            return Some(VolumeFillMatchEndReason::NoMoreBids);
        };
        // Get the ask order
        let Some(ask) = Self::next_order(
            false,
            &self.ask_idx,
            &self.debt,
            self.amm_price.as_ref(),
            self.book.asks(),
            &self.ask_outcomes
        ) else {
            return Some(VolumeFillMatchEndReason::NoMoreAsks)
        };

        println!("------RAW ORDERS------\n{:?}\n{:?}\n----------------------", bid, ask);

        // Check to see if we've hit an end state
        // If we're talking to the AMM on both sides, we're done
        if bid.is_amm() && ask.is_amm() {
            return Some(VolumeFillMatchEndReason::BothSidesAMM)
        }

        // If our prices no longer cross, we're done
        if ask.price() > bid.price() {
            return Some(VolumeFillMatchEndReason::NoLongerCross)
        }

        // Limit to price so that AMM orders will only offer the quantity they can
        // profitably sell.  (Non-AMM orders ignore the provided price)
        let ask_q = ask.quantity(bid.price());
        let bid_q = bid.quantity(ask.price());

        println!("Got bid: {} @ {:?}", bid_q, bid.price());
        println!("Got ask: {} @ {:?}", ask_q, ask.price());

        // Check to see if we have a 0-quantity ask and need to do an ask-side fill
        // This is only applicable if our ask order has the debt in it
        if ask_q == 0 && ask.is_debt() {
            println!("Doing ask-side backmatch");
            let Some(next_ask) = Self::next_order(
                false,
                &self.ask_idx,
                // Deliberately no debt here, we want what the next available order would be
                // WITHOUT our debt
                &None,
                self.amm_price.as_ref(),
                self.book.asks(),
                &self.ask_outcomes
            ) else {
                return Some(VolumeFillMatchEndReason::NoMoreAsks);
            };

            // If we don't have a valid ask order to do an ask-side fill, we are done
            if next_ask.price() > bid.price() {
                return Some(VolumeFillMatchEndReason::NoLongerCross);
            }

            // Check to see if our next order is AMM.  If so we have to do some cool
            // bounding math where we reset the bound of our current order to be
            // the closer of the intersection point or the next order's bound.
            let normal_next_q = next_ask.quantity(bid.price());
            let next_ask_q = if next_ask.is_amm() {
                self.debt
                    .as_ref()
                    .and_then(|d| {
                        next_ask
                            .amm_intersect(*d)
                            .ok()
                            .map(|i| std::cmp::min(i, normal_next_q))
                    })
                    .unwrap_or(normal_next_q)
            } else {
                normal_next_q
            };
            // Get the quantity of the debt on the current composite bid
            let cur_ask_q = ask.negative_quantity(bid.price());

            if cur_ask_q == 0 {
                println!("No positive quantity, but no negative quantity?");
                return Some(VolumeFillMatchEndReason::ErrorEncountered);
            }

            let matched = next_ask_q.min(cur_ask_q);

            // Move the AMM if we have matched against an AMM order
            if ask.is_amm() || next_ask.is_amm() {
                if let Some(amm) = self.amm_price.as_mut() {
                    if Self::fill_amm(
                        amm,
                        &mut self.results,
                        &mut self.amm_outcome,
                        matched,
                        Direction::SellingT0
                    )
                    .is_err()
                    {
                        return Some(VolumeFillMatchEndReason::ErrorEncountered);
                    }
                }
            }

            match next_ask_q.cmp(&cur_ask_q) {
                Ordering::Equal => {
                    println!("Equal match");
                    // We annihilated
                    self.results.price = Some(next_ask.price());
                    // Mark as filled if non-AMM order
                    if !next_ask.is_amm() && !next_ask.is_composite() {
                        self.ask_outcomes[self.ask_idx.get()] = OrderFillState::CompleteFill
                    }
                    // Set the Debt's current price to the target price
                    self.debt = self.debt.map(|d| d.set_price(next_ask.price().into()));
                    // Take a snapshot as a good solve state
                    self.save_checkpoint();
                }
                Ordering::Greater => {
                    println!("Greater match");
                    // Our next order is greater than our debt
                    // The end point is our next ask's price
                    self.results.price = Some(next_ask.price());
                    // Set the Debt's current price to the target price
                    self.debt = self.debt.map(|d| d.set_price(next_ask.price().into()));
                    // Set our order outcome as partially filled
                    if !next_ask.is_amm() && !next_ask.is_composite() {
                        self.ask_outcomes[self.ask_idx.get()] =
                            self.ask_outcomes[self.ask_idx.get()].partial_fill(matched);
                    }
                }
                Ordering::Less => {
                    println!("Less match");
                    self.results.price = Some(bid.price());
                    // Our debt is greater than the order
                    // Find the end price of the debt and move it there
                    self.debt = self.debt.map(|d| d.partial_fill(matched));
                    // Mark as filled if non-AMM order
                    if !next_ask.is_amm() && !next_ask.is_composite() {
                        self.ask_outcomes[self.ask_idx.get()] = OrderFillState::CompleteFill
                    }
                    // This is a good solve state
                    self.save_checkpoint();
                }
            }
            // Start the matching process again
            return None;
        }

        println!("Normal match of bid_q: {} vs ask_q: {}", bid_q, ask_q);

        // If either quantity is zero at this point we should break
        if ask_q == 0 || bid_q == 0 {
            return Some(VolumeFillMatchEndReason::ZeroQuantity)
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
        if let Some(amm) = self.amm_price.as_mut() {
            let direction = match (bid.is_amm(), ask.is_amm()) {
                (true, false) => Some(Direction::BuyingT0),
                (false, true) => Some(Direction::SellingT0),
                (..) => None
            };
            if let Some(d) = direction {
                if Self::fill_amm(amm, &mut self.results, &mut self.amm_outcome, matched, d)
                    .is_err()
                {
                    return Some(VolumeFillMatchEndReason::ErrorEncountered);
                }
            }
        }

        // Then we see what else we need to do
        match bid_q.cmp(&ask_q) {
            Ordering::Equal => {
                println!("Equal match");
                // We annihilated
                self.results.price = Some((*(ask.price() + bid.price()) / U256::from(2)).into());
                // self.results.price = Some((ask.price() + bid.price()) / 2.0_f64);
                // Mark as filled if non-AMM order
                if !ask.is_amm() && !ask.is_composite() {
                    self.ask_outcomes[self.ask_idx.get()] = OrderFillState::CompleteFill
                }
                if !bid.is_amm() && !ask.is_composite() {
                    self.bid_outcomes[self.bid_idx.get()] = OrderFillState::CompleteFill
                }
                // Take a snapshot as a good solve state
                self.save_checkpoint();
                // We're done here, we'll get our next bid and ask on
                // the next round
            }
            Ordering::Greater => {
                println!("Greater than match");
                self.results.price = Some(bid.price());
                // Ask was completely filled, remainder bid
                if !ask.is_amm() && !ask.is_composite() {
                    self.ask_outcomes[self.ask_idx.get()] = OrderFillState::CompleteFill
                }
                // Set our bid outcome to be partial
                if !bid.is_amm() && !bid.is_composite() {
                    self.bid_outcomes[self.bid_idx.get()] =
                        self.bid_outcomes[self.bid_idx.get()].partial_fill(matched);
                    // A partial fill of a partial-safe order is checkpointable
                    if bid.is_partial() {
                        self.save_checkpoint();
                    }
                } else {
                    // A partial fill of any non-book order is checkpointable
                    self.save_checkpoint();
                }
            }
            Ordering::Less => {
                println!("Less than match");
                self.results.price = Some(ask.price());
                // Bid was completely filled, remainder ask
                if !bid.is_amm() && !bid.is_composite() {
                    self.bid_outcomes[self.bid_idx.get()] = OrderFillState::CompleteFill
                }
                // Set our ask outcome to be partial
                if !ask.is_amm() && !ask.is_composite() {
                    self.ask_outcomes[self.ask_idx.get()] =
                        self.ask_outcomes[self.ask_idx.get()].partial_fill(matched);
                    // A partial fill of a partial-safe order is checkpointable
                    if ask.is_partial() {
                        self.save_checkpoint();
                    }
                } else {
                    // A partial fill of any non-book order is checkpointable
                    self.save_checkpoint();
                }
            }
        }
        // Everything went well and we have no reason to stop
        None
    }

    fn next_order(
        bid: bool,
        book_idx: &Cell<usize>,
        debt: &Option<Debt>,
        amm: Option<&PoolPrice<'a>>,
        book: &'a [BookOrder],
        fill_state: &[OrderFillState]
    ) -> Option<OrderContainer<'a>> {
        println!("Getting next order for bid {} and debt {:?}", bid, debt);
        // If we have a fragment, that takes priority
        if let Some(state @ OrderFillState::PartialFill(_)) = fill_state.get(book_idx.get()) {
            return book
                .get(book_idx.get())
                .map(|order| OrderContainer::BookOrderFragment { order, state: *state })
        }
        // Fix what makes a price "less" or "more" advantageous depending on direction
        let (less_advantageous, more_advantageous) = if bid {
            // If it's a bid, a lower price is less advantageous and a higher price is more
            // advantageous
            (Ordering::Less, Ordering::Greater)
        } else {
            // If it's an ask, a higher price is less advantageous and a lower price is more
            // advantageous
            (Ordering::Greater, Ordering::Less)
        };
        let mut cur_idx = book_idx.get();
        while cur_idx < fill_state.len() {
            if let OrderFillState::Unfilled = fill_state[cur_idx] {
                break;
            }
            cur_idx += 1;
        }
        let book_order = book.get(cur_idx);

        // If we have some debt that is at a better price, then we're going to be making
        // a debt order
        if let Some(d) = debt {
            // Compare our debt to our book price, debt is more advantageous if there's no
            // book order
            let debt_book_cmp = book_order
                .map(|b| d.price().cmp(&b.price_for_book_side(bid)))
                .unwrap_or(more_advantageous);
            // Compare our debt to our AMM, debt is more advantageous if there's no AMM
            let debt_amm_cmp = amm
                .map(|a| d.partial_cmp(a).unwrap())
                .unwrap_or(more_advantageous);

            match (debt_book_cmp, debt_amm_cmp) {
                // If the debt is less advantageous (Not sure how that could happen?) or equal to
                // the book, we should prioritize making a book order
                (dbc, _) if dbc == less_advantageous => (),
                (Ordering::Equal, _) => (),
                // Debt == AMM -> CompositeOrder(Debt, Amm) bound to the next book order
                (_, Ordering::Equal) => {
                    let bound_price = book_order.map(|b| b.price_for_book_side(bid));
                    return Some(OrderContainer::Composite(CompositeOrder::new(
                        *debt,
                        amm.cloned(),
                        bound_price
                    )))
                }
                // Debt > AMM -> CompositeOrder(Debt), bound to the closer of the AMM or the next
                // book order
                (_, dac) if dac == more_advantageous => {
                    let bound_price = book_order
                        .map(|b| {
                            amm.map(|a| max(b.price_for_book_side(bid), a.as_ray()))
                                .unwrap_or_else(|| b.price_for_book_side(bid))
                        })
                        .or_else(|| amm.map(|a| a.as_ray()));
                    return Some(OrderContainer::Composite(CompositeOrder::new(
                        *debt,
                        None,
                        bound_price
                    )))
                }
                _ => panic!("Debt should never be on the wrong side of the AMM")
            }
        }

        // If we have an AMM price, see if it takes precedence over our book order
        amm.and_then(|a| {
            println!("Comparing AMM to book order");
            let bound_price = if let Some(o) = book_order {
                println!(
                    "AMM price: {:?}\nBook price: {:?}",
                    a.as_ray(),
                    o.price_for_book_side(bid)
                );
                if o.price_for_book_side(bid).cmp(&a.as_ray()) != less_advantageous {
                    println!("Book order better than AMM price");
                    return None
                }
                Some(o.price_for_book_side(bid))
            } else {
                None
            };
            // Otherwise, my AMM price is better than my book price and we should make an
            // AMM order
            Some(CompositeOrder::new(None, Some(a.clone()), bound_price))
        })
        .map(OrderContainer::Composite)
        .or_else(|| {
            book_idx.set(cur_idx);
            book_order.map(OrderContainer::BookOrder)
        })
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
            .map(|(id, outcome)| OrderOutcome { id, outcome: *outcome })
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
    use std::{cell::Cell, cmp::max};

    use alloy::primitives::Uint;
    use alloy_primitives::FixedBytes;
    use angstrom_types::{
        matching::{uniswap::PoolSnapshot, Debt, DebtType, Ray, SqrtPriceX96},
        orders::OrderFillState,
        primitive::PoolId
    };
    use testing_tools::type_generator::{
        amm::generate_single_position_amm_at_tick, orders::UserOrderBuilder
    };

    use super::VolumeFillMatcher;
    use crate::book::{order::OrderContainer, BookOrder, OrderBook};

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
        let bid_price = Ray::from(Uint::from(1_000_000_000_u128)).inv_ray_round(true);
        let low_price = Ray::from(Uint::from(1_000_u128));
        let bid_order = UserOrderBuilder::new()
            .partial()
            .amount(100)
            .min_price(bid_price)
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
        println!("Ask order:\n{:?}", ask_order);
        let book = OrderBook::new(pool_id, None, vec![bid_order.clone()], vec![ask_order], None);
        let mut matcher = VolumeFillMatcher::new(&book);
        let _fill_outcome = matcher.run_match();
        let solution = matcher.from_checkpoint().unwrap().solution(None);
        assert!(
            solution.ucp == bid_price.inv_ray_round(true),
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
            .bid_min_price(high_price)
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
        let _fill_outcome = matcher.run_match();
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
    ) -> (Vec<BookOrder>, Vec<OrderFillState>) {
        let orders = (0..count)
            .map(|i| {
                // Step downwards if it's a bid to simulate bid book ordering
                let min_price = if is_bid {
                    (target_price - (i * price_step)).inv_ray_round(true)
                } else {
                    target_price + (i * price_step)
                };
                UserOrderBuilder::new()
                    .min_price(min_price)
                    .amount(100)
                    .with_storage()
                    .is_bid(is_bid)
                    .build()
            })
            .collect();
        let states = (0..count).map(|_| OrderFillState::Unfilled).collect();
        (orders, states)
    }

    #[test]
    fn gets_next_bid_order() {
        let index = Cell::new(0);
        let (book, fill_state) = basic_order_book(true, 10, Ray::from(10000_usize), 10);
        let debt = None;
        let amm = None;
        let next_order =
            VolumeFillMatcher::next_order(true, &index, &debt, amm, &book, &fill_state).unwrap();
        if let OrderContainer::BookOrder(o) = next_order {
            assert_eq!(*o, book[0], "Next order selected was not first order in book");
        } else {
            panic!("Next order is not a BookOrder");
        }
    }

    #[test]
    fn bid_side_amm_overrides_book_order() {
        let market: PoolSnapshot =
            generate_single_position_amm_at_tick(100000, 100, 1_000_000_000_000_000_u128);
        let amm_price = market.current_price();
        let amm = Some(&amm_price);
        let debt = None;
        let index = Cell::new(0);
        let (book, fill_state) =
            basic_order_book(true, 10, Ray::from(SqrtPriceX96::at_tick(99999).unwrap()), 10);

        let next_order =
            VolumeFillMatcher::next_order(true, &index, &debt, amm, &book, &fill_state).unwrap();

        assert!(matches!(next_order, OrderContainer::Composite(_)), "Composite order not created!");
        if let OrderContainer::Composite(c) = next_order {
            println!("Order: {:?}", c);
            assert_eq!(c.start_price(), amm_price.as_ray(), "AMM price is not starting price");
            assert!(c.quantity(book[0].price()) > 0, "Composite order has zero quantity");
        } else {
            panic!("Composite order not created but did match?");
        }
    }

    #[test]
    fn bid_side_debt_overrides_amm_and_book() {
        let market: PoolSnapshot =
            generate_single_position_amm_at_tick(100000, 100, 1_000_000_000_000_000_u128);
        let amm_price = market.current_price();
        let amm = Some(&amm_price);
        let debt = Some(Debt::new(
            DebtType::ExactIn(100000000),
            Ray::from(SqrtPriceX96::at_tick(101001).unwrap())
        ));
        let index = Cell::new(0);
        let (book, fill_state) =
            basic_order_book(true, 10, Ray::from(SqrtPriceX96::at_tick(99999).unwrap()), 10);

        let next_order =
            VolumeFillMatcher::next_order(true, &index, &debt, amm, &book, &fill_state).unwrap();
        let order_q_target = max(book[0].price(), amm_price.as_ray());

        assert!(matches!(next_order, OrderContainer::Composite(_)), "Composite order not created!");
        if let OrderContainer::Composite(c) = next_order {
            assert!(c.debt().is_some(), "No debt in created Composite");
            assert!(c.amm().is_none(), "AMM erroneously included in created Composite");
            assert!(c.bound().is_some(), "No bound price included");
            assert!(c.quantity(order_q_target) > 0, "Composite order has zero quantity");
            assert_eq!(c.bound().unwrap(), amm_price.as_ray(), "Bound is not AMM price");
        } else {
            panic!("Composite order not created but did match?");
        }
    }

    #[test]
    fn bid_side_book_overrides_amm_and_debt() {
        let market: PoolSnapshot =
            generate_single_position_amm_at_tick(100000, 100, 1_000_000_000_000_000_u128);
        let amm_price = market.current_price();
        let amm = Some(&amm_price);
        let debt = Some(Debt::new(
            DebtType::ExactIn(100000000),
            Ray::from(SqrtPriceX96::at_tick(10001).unwrap())
        ));
        let index = Cell::new(0);
        let (book, fill_state) =
            basic_order_book(true, 10, Ray::from(SqrtPriceX96::at_tick(100100).unwrap()), 10);

        let next_order =
            VolumeFillMatcher::next_order(true, &index, &debt, amm, &book, &fill_state).unwrap();

        assert!(matches!(next_order, OrderContainer::BookOrder(_)), "Book order not chosen");
        if let OrderContainer::BookOrder(b) = next_order {
            assert_eq!(*b, book[0], "First book order not chosen");
        } else {
            panic!("Book order not created but did match?");
        }
    }

    #[test]
    fn bid_side_debt_overrides_amm_and_book_with_book_bound() {
        let market: PoolSnapshot =
            generate_single_position_amm_at_tick(99999, 100, 1_000_000_000_000_000_u128);
        let amm_price = market.current_price();
        let amm = Some(&amm_price);
        let debt = Some(Debt::new(
            DebtType::ExactIn(100000000),
            Ray::from(SqrtPriceX96::at_tick(101001).unwrap())
        ));
        let index = Cell::new(0);
        let (book, fill_state) =
            basic_order_book(true, 10, Ray::from(SqrtPriceX96::at_tick(100000).unwrap()), 10);

        let next_order =
            VolumeFillMatcher::next_order(true, &index, &debt, amm, &book, &fill_state).unwrap();

        let order_q_target = max(book[0].price(), amm_price.as_ray());

        assert!(matches!(next_order, OrderContainer::Composite(_)), "Composite order not created!");
        if let OrderContainer::Composite(c) = next_order {
            assert!(c.debt().is_some(), "No debt in created Composite");
            assert!(c.amm().is_none(), "AMM erroneously included in created Composite");
            assert!(c.bound().is_some(), "No bound price included");
            assert!(c.quantity(order_q_target) > 0, "Composite order has zero quantity");
            assert_eq!(c.bound().unwrap(), amm_price.as_ray(), "Bound is not AMM price");
        } else {
            panic!("Composite order not created but did match?");
        }
    }

    #[test]
    fn ask_side_debt_has_zero_quantity() {
        let debt = Some(Debt::new(
            DebtType::ExactIn(100000000),
            Ray::from(SqrtPriceX96::at_tick(100000).unwrap())
        ));
        let index = Cell::new(0);
        let (book, fill_state) =
            basic_order_book(false, 10, Ray::from(SqrtPriceX96::at_tick(101000).unwrap()), 10);

        let next_order =
            VolumeFillMatcher::next_order(false, &index, &debt, None, &book, &fill_state).unwrap();

        assert!(matches!(next_order, OrderContainer::Composite(_)), "Composite order not created!");
        if let OrderContainer::Composite(c) = next_order {
            let q = c.quantity(book[0].price_for_book_side(false));
            assert_eq!(q, 0, "Ask-side debt doesn't have a zero quantity!");
        } else {
            panic!("Composite order not created but did match?");
        }
    }

    #[test]
    fn ask_side_double_match_works() {
        let debt_price = Ray::from(SqrtPriceX96::at_tick(90000).unwrap());
        let ask_target_price = Ray::from(SqrtPriceX96::at_tick(100000).unwrap());
        let bid_target_price = Ray::from(SqrtPriceX96::at_tick(110000).unwrap());
        let debt = Some(Debt::new(DebtType::ExactIn(100000), debt_price));
        if let Some(ref d) = debt {
            assert!(!d.valid_for_price(ask_target_price), "Debt already at ask price");
        }
        let (ask_book, _) = basic_order_book(false, 10, ask_target_price, 10);
        let (bid_book, _) = basic_order_book(true, 10, bid_target_price, 10);

        let ob = OrderBook::new(
            FixedBytes::random(),
            None,
            bid_book,
            ask_book,
            Some(crate::book::sort::SortStrategy::ByPriceByVolume)
        );
        let mut matcher = VolumeFillMatcher::new(&ob);
        matcher.debt = debt;
        let first_ask = matcher.book.asks().get(matcher.ask_idx.get()).unwrap();
        assert!(
            !debt.as_ref().unwrap().valid_for_price(first_ask.price()),
            "Debt starting at first ask price"
        );
        let end = matcher.single_match();
        println!("Fill ended: {:?}", end);
        let current_ask = matcher
            .book
            .asks()
            .get(matcher.bid_idx.get())
            .expect("Missing current ask");
        let current_ask_fill_state = matcher
            .ask_outcomes
            .get(matcher.ask_idx.get())
            .expect("Missing current ask fill state");
        assert!(
            matches!(current_ask_fill_state, OrderFillState::PartialFill(8)),
            "Wrong amount of volume taken from our order"
        );
        assert!(matcher.debt.is_some(), "No debt left on the matcher");
        let md = matcher.debt.as_ref().unwrap();
        assert!(md.valid_for_price(current_ask.price()), "Debt is not at the current order price");

        matcher.single_match();

        let current_bid_fill_state = matcher
            .bid_outcomes
            .get(matcher.bid_idx.get())
            .expect("Missing current bid fill state");
        assert!(
            matches!(current_bid_fill_state, OrderFillState::PartialFill(92)),
            "Wrong amount of volume taken from our order"
        );
    }

    #[test]
    fn ask_side_double_match_works_with_amm() {
        let market: PoolSnapshot =
            generate_single_position_amm_at_tick(91000, 100, 1_000_000_000_000_000_u128);
        let debt_price = Ray::from(SqrtPriceX96::at_tick(90000).unwrap());
        let ask_target_price = Ray::from(SqrtPriceX96::at_tick(100000).unwrap());
        let bid_target_price = Ray::from(SqrtPriceX96::at_tick(110000).unwrap());
        let debt = Some(Debt::new(DebtType::ExactIn(100000), debt_price));
        if let Some(ref d) = debt {
            assert!(!d.valid_for_price(ask_target_price), "Debt already at ask price");
        }
        let (ask_book, _) = basic_order_book(false, 10, ask_target_price, 10);
        let (bid_book, _) = basic_order_book(true, 10, bid_target_price, 10);

        let ob = OrderBook::new(
            FixedBytes::random(),
            Some(market),
            bid_book,
            ask_book,
            Some(crate::book::sort::SortStrategy::ByPriceByVolume)
        );
        let mut matcher = VolumeFillMatcher::new(&ob);
        matcher.debt = debt;
        let first_ask = matcher.book.asks().get(matcher.ask_idx.get()).unwrap();
        assert!(
            !debt.as_ref().unwrap().valid_for_price(first_ask.price()),
            "Debt starting at first ask price"
        );
        let end = matcher.single_match();
        println!("Fill ended: {:?}", end);
    }
}
