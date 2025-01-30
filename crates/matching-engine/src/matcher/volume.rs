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
use base64::Engine;
use tracing::{trace, debug, info};

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
        info!(?bid_cnt, ?ask_cnt, "Book size");
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

    pub fn cur_debt(&self) -> Option<&Debt> {
        self.debt.as_ref()
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
        debug!(quantity, direction = ?direction, "Executing AMM fill");
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
        // Output our book data so we can do stuff with it
        let json = serde_json::to_string(self.book).unwrap();
        let b64_output = base64::prelude::BASE64_STANDARD.encode(json.as_bytes());
        trace!(data = b64_output, "Raw book data");
        // Run our match over and over until we get an end reason
        let mut i: usize = 0;
        loop {
            if let Some(r) = self.single_match() {
                tracing::debug!(?r);
                return r
            }
            i += 1;
            if i > 1000 {
                panic!("100 iterations!");
            }
        }
    }

    pub fn single_match(&mut self) -> Option<VolumeFillMatchEndReason> {
        tracing::info!("single match");
        // Get the bid order
        let Some(bid) = Self::next_order(
            true,
            &self.bid_idx,
            &mut self.debt,
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
            &mut self.debt,
            self.amm_price.as_ref(),
            self.book.asks(),
            &self.ask_outcomes
        ) else {
            return Some(VolumeFillMatchEndReason::NoMoreAsks)
        };

        debug!(bid = ?bid, ask = ?ask, "Raw orders");

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
        // These quantities might be in T0 or T1 depending, we might want to be a bit
        // more explicit about this, but they will always be in the SAME amount
        let (bid_q, ask_q) = Self::get_match_quantities(&bid, &ask, self.debt.as_ref());

        debug!(bid_q, ask_q, bid_price = ?bid.price(), ask_price = ?ask.price(), "Bid and ask stats");

        // Check to see if we have a 0-quantity ask and need to do an ask-side fill
        // This is only applicable if our ask order has the debt in it
        if ask_q == 0 && ask.is_debt() {
            debug!("Executing ask-side backmatch");

            // Ind our next available order
            let Some(next_ask) = Self::next_order(
                false,
                &self.ask_idx,
                // Deliberately no debt here, we want what the next available order would be
                // WITHOUT our debt
                &mut None,
                self.amm_price.as_ref(),
                self.book.asks(),
                &self.ask_outcomes
            ) else {
                return Some(VolumeFillMatchEndReason::NoMoreAsks);
            };

            debug!(original = ?ask, next = ?next_ask, "Orders for ask-side backmatch");

            // First we check if we have a combination AMM/Debt Composite order.  If we do,
            // we're here because the AMM can't provide more T0 than it costs to move the
            // debt.  In this case, we should use all the AMM liquidity possible to move the
            // debt FIRST before we do any kind of backmatch
            if ask.is_amm() {
                debug!("Composite is combination AMM and Debt");
                // Move the AMM
                let (amm_q, _) = ask.composite_quantities_to_price(next_ask.price());
                if let Some(amm) = self.amm_price.as_mut() {
                    if Self::fill_amm(
                        amm,
                        &mut self.results,
                        &mut self.amm_outcome,
                        amm_q,
                        Direction::BuyingT0
                    )
                    .is_err()
                    {
                        return Some(VolumeFillMatchEndReason::ErrorEncountered);
                    }
                }

                // Update the debt
                self.debt = self.debt.map(|d| d.partial_fill(amm_q));

                debug!(quantity = amm_q, "Sold quantity from the AMM into the debt");

                // Start a new cycle
                return None;
            }

            // If we don't have a valid ask order to do an ask-side fill, we are done
            if next_ask.price() > bid.price() {
                return Some(VolumeFillMatchEndReason::NoLongerCross);
            }

            // If our next order is an AMM but the AMM is already our best bid-side order,
            // we are done
            if next_ask.is_amm() && bid.is_amm() {
                return Some(VolumeFillMatchEndReason::NoMoreAsks);
            }

            // Determine if we're going to backmatch in a T1 context - this is true if our
            // first ask is a solo debt order and our next ask is an inverse order
            let t1_context = !ask.is_amm() && next_ask.inverse_order();

            // Check to see if our next order is AMM.  If so we have to do some cool
            // bounding math where we reset the bound of our current order to be
            // the closer of the intersection point or the next order's bound.
            let normal_next_q = next_ask.quantity(&bid, self.debt.as_ref());
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
            let cur_ask_q = if t1_context {
                ask.negative_quantity_t1(bid.price())
            } else {
                ask.negative_quantity(bid.price())
            };

            if cur_ask_q == 0 {
                debug!("No positive quantity but no negative quantity");
                return Some(VolumeFillMatchEndReason::ErrorEncountered);
            }

            debug!(current = cur_ask_q, next = next_ask_q, "Backmatch quantities");

            let matched = next_ask_q.min(cur_ask_q);

            // If we matched nothing, we should end
            if matched == 0 {
                return Some(VolumeFillMatchEndReason::ZeroQuantity);
            }

            // Move the AMM if we have matched against an AMM order
            if ask.is_amm() || next_ask.is_amm() {
                if let Some(amm) = self.amm_price.as_mut() {
                    if Self::fill_amm(
                        amm,
                        &mut self.results,
                        &mut self.amm_outcome,
                        matched,
                        Direction::BuyingT0
                    )
                    .is_err()
                    {
                        return Some(VolumeFillMatchEndReason::ErrorEncountered);
                    }
                }
            }

            match next_ask_q.cmp(&cur_ask_q) {
                Ordering::Equal => {
                    debug!("Equal match quantities");
                    // We annihilated in which case the debt price has moved to the next_ask price
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
                    debug!("Next ask greater than current ask");
                    // Our next order is greater than our debt.  The debt has been moved to next_ask
                    // price without consuming the entirety of next_ask
                    // The end point is our next ask's price
                    self.results.price = Some(next_ask.price());
                    // Set the Debt's current price to the target price
                    self.debt = self.debt.map(|d| d.set_price(next_ask.price().into()));
                    // Set our order outcome as partially filled
                    if !next_ask.is_amm() && !next_ask.is_composite() {
                        self.ask_outcomes[self.ask_idx.get()] =
                            self.ask_outcomes[self.ask_idx.get()].partial_fill(matched);
                    }
                    // This is not a valid end state because next_ask is not
                    // completely filled
                }
                Ordering::Less => {
                    debug!("Next ask less than current ask");
                    // Our debt is greater than the order
                    // Find the end price of the debt and move it there
                    if let Some(cur_debt) = self.debt.as_mut() {
                        let new_debt = cur_debt.partial_fill(matched);
                        // Our new final price is the last moved price of our debt
                        self.results.price = Some(new_debt.price().into());
                        *cur_debt = new_debt;
                    }
                    // Mark as filled if non-AMM order
                    if !next_ask.is_amm() && !next_ask.is_composite() {
                        self.ask_outcomes[self.ask_idx.get()] = OrderFillState::CompleteFill
                    }
                    // This is NOT a good solve state - if we didn't backfill
                    // all the way we are unstable beacuse our final price isn't
                    // as good as the price we backfilled from (making the
                    // transaction invalid)
                }
            }
            // Start the matching process again
            return None;
        }

        debug!(bid_quantity = bid_q, ask_quantity = ask_q, "Executing normal match");

        // If either quantity is zero at this point we should break
        // I actually think this might be OK - there are some edge cases where this is
        // fine
        // A 0-volume match can happen if we have some kind of "slack" bid or ask left
        if ask_q == 0 || bid_q == 0 {
            return Some(VolumeFillMatchEndReason::ZeroQuantity)
        }

        // Determine how much we matched and if our orders totally annihilated
        let matched = ask_q.min(bid_q);
        debug!(matched, "Mathed normal quantity");

        // --- Instrumentation for benchmarking needs updating ---
        // Store the amount we matched
        self.results.total_volume += matched;

        // Record partial fills
        if bid.is_partial() {
            self.results.partial_volume.0 += matched;
        }
        if ask.is_partial() {
            self.results.partial_volume.1 += matched;
        }
        // --- End instrumentation ---

        // Time to update our AMM and/or debt based on our match
        let t1_context =
            (bid.inverse_order() || bid.is_debt()) && (ask.inverse_order() || ask.is_debt());

        // Find our AMM order
        let amm_order = if bid.is_amm() {
            Some((&bid, Direction::SellingT0))
        } else if ask.is_amm() {
            Some((&ask, Direction::BuyingT0))
        } else {
            None
        };

        // Update our AMM from our AMM order if we have one
        if let Some((a_o, direction)) = amm_order {
            if let Some(amm) = self.amm_price.as_mut() {
                // We shouldn't be in a t1 context unless a_o.is_debt() is true, but let's be
                // explicit
                let quantity = if t1_context && a_o.is_debt() {
                    // Move the AMM by the amount of T0 "freed" from the debt
                    self.debt.unwrap().freed_t0(matched)
                } else {
                    // Move the AMM by the portion of the matched T0
                    // Can unwrap here as we've checked to be sure the order is valid
                    let quantities = a_o.composite_t0_quantities(matched, direction);
                    debug!(quantities = ?quantities, "Found mixed quantities");
                    quantities.0.unwrap()
                };
                if Self::fill_amm(
                    amm,
                    &mut self.results,
                    &mut self.amm_outcome,
                    quantity,
                    direction
                )
                .is_err()
                {
                    return Some(VolumeFillMatchEndReason::ErrorEncountered);
                }
            }
        }

        // Find our partial match quantity if we need our debt to calculate that.  We
        // have to do this before we adjust the debt because it relies on the current
        // debt
        let t1_matched = if t1_context {
            matched
        } else {
            // Our matched quantity is in T0 so we have to convert it into the appropriate
            // T1 quantity for our book order
            match (bid.inverse_order(), ask.inverse_order()) {
                // For an inverse bid the listed quantity is the T1
                (true, false) => bid
                    .max_t1_for_t0(matched, self.debt.as_ref())
                    .expect("Somehow no T1 available"),
                // For an inverse ask the listed quantity is the
                (false, true) => ask
                    .max_t1_for_t0(matched, self.debt.as_ref())
                    .expect("Somehow no T1 available"),
                _ => 0
            }
        };

        // Adjust our debt
        // Find our debt order
        let debt_order = if bid.is_debt() {
            Some((&bid, Direction::SellingT0))
        } else if ask.is_debt() {
            Some((&ask, Direction::BuyingT0))
        } else {
            None
        };

        // Update the debt based on whether we have a debt order or not
        if let Some((d_o, direction)) = debt_order {
            if t1_context {
                if let Some(d) = self.debt.as_mut() {
                    if d_o.is_amm() {
                        // If the AMM is here, we've used the T1 to feed T0 into the AMM.  Maybe
                        // we've done this at the AMM step?
                        *d = d.flat_fill_t1(matched);
                    } else {
                        // In t1 context, if we don't have the AMM in this order, we use the T1
                        // difference to adjust the price.
                        *d = d.partial_fill_t1(matched);
                    }
                }
            } else {
                let quantity = d_o.composite_t0_quantities(matched, direction).1.unwrap();
                if let Some(d) = self.debt.as_mut() {
                    *d = d.partial_fill(quantity);
                }
            }
        } else {
            // Without a debt order we can alwasys just add the total of our
            // compared orders' debts - if both have debt it annihilates and if neither does
            // we don't need to do it
            if let Some(net_debt) = ask
                .as_debt(Some(t1_matched), false)
                .xor(bid.as_debt(Some(t1_matched), true))
            {
                debug!(limit_t1 = t1_matched, net_debt = ?net_debt, "Adding net debt");
                self.debt += net_debt;
            }
        }

        debug!(debt = ?self.debt, "Current debt");

        // Then we deal with fixing up our book orders
        match bid_q.cmp(&ask_q) {
            Ordering::Equal => {
                debug!("Equal match");
                // We annihilated

                // If we have a debt price, this is our current price, otherwise we get a price
                // from our order outcomes
                let new_price = self
                    .debt
                    .map(|d| d.price())
                    .unwrap_or_else(|| (*(ask.price() + bid.price()) / U256::from(2)).into());
                self.results.price = Some(new_price.into());

                // Mark book orders as CompletelyFilled
                if ask.is_book() {
                    self.ask_outcomes[self.ask_idx.get()] = OrderFillState::CompleteFill
                }
                if bid.is_book() {
                    self.bid_outcomes[self.bid_idx.get()] = OrderFillState::CompleteFill
                }

                // Take a snapshot as a good solve state
                self.save_checkpoint();
                // We're done here, we'll get our next bid and ask on
                // the next round
            }
            Ordering::Greater => {
                debug!("Greater than match");
                self.results.price = Some(bid.price());
                // Ask was completely filled, remainder bid
                if ask.is_book() {
                    self.ask_outcomes[self.ask_idx.get()] = OrderFillState::CompleteFill
                }
                // Set our bid outcome to be partial
                if bid.is_book() {
                    let partial_q = if bid.inverse_order() { t1_matched } else { matched };
                    self.bid_outcomes[self.bid_idx.get()] =
                        self.bid_outcomes[self.bid_idx.get()].partial_fill(partial_q);
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
                debug!("Less than match");
                self.results.price = Some(ask.price());
                // Bid was completely filled, remainder ask
                if bid.is_book() {
                    self.bid_outcomes[self.bid_idx.get()] = OrderFillState::CompleteFill
                }
                // Set our ask outcome to be partial
                if ask.is_book() {
                    let partial_q = if ask.inverse_order() { t1_matched } else { matched };
                    self.ask_outcomes[self.ask_idx.get()] =
                        self.ask_outcomes[self.ask_idx.get()].partial_fill(partial_q);
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

    /// Returns (bid_q, ask_q)
    fn get_match_quantities(
        bid: &OrderContainer,
        ask: &OrderContainer,
        debt: Option<&Debt>
    ) -> (u128, u128) {
        if bid.is_book() && ask.is_book() {
            // We have a pair of book orders
            match (bid.inverse_order(), ask.inverse_order()) {
                // Inverse vs inverse is a T1 match
                (true, true) => {
                    // We already know these are book orders so we can unwrap here
                    (bid.quantity_t1(debt).unwrap(), ask.quantity_t1(debt).unwrap())
                }
                // Mixed order returns quantity in T0 at debt or order price
                (true, false) | (false, true) => (bid.quantity(ask, debt), ask.quantity(bid, debt)),
                // Normal book order and normal book order just return T0 quantities
                (false, false) => (bid.quantity(ask, debt), ask.quantity(bid, debt))
            }
        } else {
            // We have either a book order and a Composite order or a pair of Composite
            // orders, all of which return T0
            (bid.quantity(ask, debt), ask.quantity(bid, debt))
        }
    }

    fn next_order(
        bid: bool,
        book_idx: &Cell<usize>,
        debt: &mut Option<Debt>,
        amm: Option<&PoolPrice<'a>>,
        book: &'a [BookOrder],
        fill_state: &[OrderFillState]
    ) -> Option<OrderContainer<'a>> {
        debug!(is_bid = bid, debt = ?debt, "Getting next order");
        // If we have a fragment, that takes priority
        if let Some(state @ OrderFillState::PartialFill(_)) = fill_state.get(book_idx.get()) {
            return book
                .get(book_idx.get())
                .map(|order| OrderContainer::BookOrder { order, state: *state })
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

        let this_side_debt = debt.filter(|d| d.bid_side() == bid);
        // If we have some debt that is at a better price, then we're going to be making
        // a debt order
        if let Some(mut d) = this_side_debt {
            // Compare our debt to our book price, debt is more advantageous if there's no
            // book order
            let debt_book_cmp = book_order
                .map(|b| {
                    let book_price = b.price_for_book_side(bid);
                    debug!(debt_price = ?d.price(), book_price = ?book_price, "Comparing debt with book price");
                    if d.validate_and_set_price(book_price) {
                        debug!("Debt and book equal");
                        Ordering::Equal
                    } else {
                        debug!("Debt and book not equal");
                        d.price().cmp(&book_price)
                    }
                })
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
                // Debt more advantageous than AMM -> CompositeOrder(Debt), bound to the closer of
                // the AMM or the next book order
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
                // Debt is more advantageous than book but less advantageous than the AMM, wherever
                // it might be
                _ => panic!(
                    "Debt should never be on the wrong side of the AMM\nDebt price: {:?}\nAMM \
                     price: {:?}",
                    debt.map(|d| d.price()),
                    amm.map(|a| Ray::from(a.price()))
                )
            }
        }

        // If we have an AMM price, see if it takes precedence over our book order
        amm.and_then(|a| {
            debug!("Comparing AMM to book");
            let bound_price = if let Some(o) = book_order {
                debug!(amm_price = ?a.as_ray(), book_price = ?o.price_for_book_side(bid), "Amm and book prices");
                if o.price_for_book_side(bid).cmp(&a.as_ray()) != less_advantageous {
                    debug!("Book order better than AMM");
                    return None
                } else {
                    debug!("AMM order better than book");
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
            book_order.map(|order| {
                let state = fill_state[cur_idx];
                OrderContainer::BookOrder { order, state }
            })
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
            .bid()
            .amount(100)
            .min_price(bid_price)
            .with_storage()
            .bid()
            .build();
        let ask_order = UserOrderBuilder::new()
            .exact()
            .ask()
            .amount(10)
            .exact_in(true)
            .min_price(low_price)
            .with_storage()
            .ask()
            .build();
        println!("Bid order:\n{:?}", bid_order);
        println!("Ask order:\n{:?}", ask_order);
        let book = OrderBook::new(pool_id, None, vec![bid_order.clone()], vec![ask_order], None);
        let mut matcher = VolumeFillMatcher::new(&book);
        let _fill_outcome = matcher.run_match();
        let solution = matcher.from_checkpoint().unwrap().solution(None);
        println!(
            "Solution UCP: {:?}\nFinal bid: {:?}",
            solution.ucp,
            bid_price.inv_ray_round(true)
        );
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
            .bid()
            .amount(10)
            .bid_min_price(high_price)
            .with_storage()
            .bid()
            .build();
        let ask_order = UserOrderBuilder::new()
            .partial()
            .ask()
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
                    .exact()
                    .exact_in(!is_bid)
                    .min_price(min_price)
                    .amount(100)
                    .is_bid(is_bid)
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
        let mut debt = None;
        let amm = None;
        let next_order =
            VolumeFillMatcher::next_order(true, &index, &mut debt, amm, &book, &fill_state)
                .unwrap();
        if let OrderContainer::BookOrder { order, .. } = next_order {
            assert_eq!(*order, book[0], "Next order selected was not first order in book");
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
        let mut debt = None;
        let index = Cell::new(0);
        let (book, fill_state) =
            basic_order_book(true, 10, Ray::from(SqrtPriceX96::at_tick(99999).unwrap()), 10);

        let next_order =
            VolumeFillMatcher::next_order(true, &index, &mut debt, amm, &book, &fill_state)
                .unwrap();

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
        let mut debt = Some(Debt::new(
            DebtType::ExactIn(100000000),
            Ray::from(SqrtPriceX96::at_tick(101001).unwrap())
        ));
        let index = Cell::new(0);
        let (book, fill_state) =
            basic_order_book(true, 10, Ray::from(SqrtPriceX96::at_tick(99999).unwrap()), 10);

        let next_order =
            VolumeFillMatcher::next_order(true, &index, &mut debt, amm, &book, &fill_state)
                .unwrap();
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
        let mut debt = Some(Debt::new(
            DebtType::ExactIn(100000000),
            Ray::from(SqrtPriceX96::at_tick(10001).unwrap())
        ));
        let index = Cell::new(0);
        let (book, fill_state) =
            basic_order_book(true, 10, Ray::from(SqrtPriceX96::at_tick(100100).unwrap()), 10);

        let next_order =
            VolumeFillMatcher::next_order(true, &index, &mut debt, amm, &book, &fill_state)
                .unwrap();

        assert!(matches!(next_order, OrderContainer::BookOrder { .. }), "Book order not chosen");
        if let OrderContainer::BookOrder { order: b, .. } = next_order {
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
        let mut debt = Some(Debt::new(
            DebtType::ExactIn(100000000),
            Ray::from(SqrtPriceX96::at_tick(101001).unwrap())
        ));
        let index = Cell::new(0);
        let (book, fill_state) =
            basic_order_book(true, 10, Ray::from(SqrtPriceX96::at_tick(100000).unwrap()), 10);

        let next_order =
            VolumeFillMatcher::next_order(true, &index, &mut debt, amm, &book, &fill_state)
                .unwrap();

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
        let mut debt = Some(Debt::new(
            DebtType::ExactOut(100000000),
            Ray::from(SqrtPriceX96::at_tick(100000).unwrap())
        ));
        let index = Cell::new(0);
        let (book, fill_state) =
            basic_order_book(false, 10, Ray::from(SqrtPriceX96::at_tick(101000).unwrap()), 10);

        let next_order =
            VolumeFillMatcher::next_order(false, &index, &mut debt, None, &book, &fill_state)
                .unwrap();

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
        let debt = Some(Debt::new(DebtType::ExactOut(100000), debt_price));
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

    #[test]
    fn get_match_quantities_works_properly() {
        let bid_price = Ray::from(SqrtPriceX96::at_tick(110000).unwrap());
        let ask_price = Ray::from(SqrtPriceX96::at_tick(100000).unwrap());
        let (bid_book, _) = basic_order_book(true, 10, bid_price, 10);
        let (ask_book, _) = basic_order_book(false, 10, ask_price, 10);
        let bid = OrderContainer::from(&bid_book[0]);
        let ask = OrderContainer::from(&ask_book[0]);
        println!("Bid order: {:?}\nAsk order: {:?}", bid, ask);
        let (bid_q, ask_q) = VolumeFillMatcher::get_match_quantities(&bid, &ask, None);
        println!("Bidq: {}\nAskq: {}", bid_q, ask_q);
    }
}
