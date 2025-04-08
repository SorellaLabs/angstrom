use std::cmp::{Ordering, min};

use alloy_primitives::{I256, U256};
use angstrom_types::{
    contract_payloads::angstrom::TopOfBlockOrder as ContractTopOfBlockOrder,
    matching::{
        SqrtPriceX96, get_quantities_at_price,
        uniswap::{Direction, PoolPrice, PoolPriceVec, Quantity}
    },
    orders::{NetAmmOrder, OrderFillState, OrderOutcome, PoolSolution},
    sol_bindings::{
        RawPoolOrder, Ray,
        grouped_orders::{GroupedVanillaOrder, OrderWithStorageData},
        rpc_orders::TopOfBlockOrder
    }
};
use serde::{Deserialize, Serialize};
use tracing::trace;

use crate::OrderBook;

/// Enum describing what kind of ToB order we want to use to set the initial AMM
/// price for our DeltaMatcher
#[derive(Debug, Serialize, Deserialize)]
pub enum DeltaMatcherToB {
    /// No ToB Order at all, no price movement
    None,
    /// Use a fixed shift in format (Quantity, is_bid), mostly for testing.  In
    /// this case is_bid == !zero_for_one in the ToB order, we then flip it
    /// around again when we resolve it because Direction::from_is_bid wants to
    /// undersand how the POOL is behaving.  This is complicated and should be
    /// cleaned up, but since it's only used for debugging right now it'll be
    /// OK.
    FixedShift(Quantity, bool),
    /// Extract the information from an actual order being fed to the matcher
    Order(OrderWithStorageData<TopOfBlockOrder>)
}

impl From<Option<OrderWithStorageData<TopOfBlockOrder>>> for DeltaMatcherToB {
    fn from(value: Option<OrderWithStorageData<TopOfBlockOrder>>) -> Self {
        match value {
            None => Self::None,
            Some(o) => Self::Order(o)
        }
    }
}

#[derive(Clone)]
pub struct DeltaMatcher<'a> {
    book:            &'a OrderBook,
    fee:             u128,
    /// If true, we solve for T0.  If false we solve for T1.
    solve_for_t0:    bool,
    /// changes if there is a tob or not
    amm_start_price: Option<PoolPrice<'a>>
}

impl<'a> DeltaMatcher<'a> {
    pub fn new(book: &'a OrderBook, tob: DeltaMatcherToB, fee: u128, solve_for_t0: bool) -> Self {
        let amm_start_price = match tob {
            // If we have an order, apply that to the AMM start price
            DeltaMatcherToB::Order(ref tob) => book.amm().map(|snapshot| {
                ContractTopOfBlockOrder::calc_vec_and_reward(tob, snapshot)
                    .expect("Order structure should be valid and never fail")
                    .0
                    .end_bound
            }),
            // If we have a fixed shift, apply that to the AMM start price (Not yet operational)
            DeltaMatcherToB::FixedShift(q, is_bid) => book.amm().and_then(|f| {
                PoolPriceVec::from_swap(f.current_price(), Direction::from_is_bid(!is_bid), q)
                    .ok()
                    .map(|v| v.end_bound)
            }),
            // If we have no order or shift, we just use the AMM start price as-is
            DeltaMatcherToB::None => book.amm().map(|f| f.current_price())
        };

        Self { book, amm_start_price, fee, solve_for_t0 }
    }

    fn fetch_concentrated_liquidity(&self, price: Ray) -> (I256, I256) {
        let Some(start_price) = self.amm_start_price.clone() else { return Default::default() };
        let Ok(end_price) = start_price.snapshot().at_price(SqrtPriceX96::from(price)) else {
            return Default::default();
        };
        let start_sqrt = start_price.as_sqrtpricex96();
        let end_sqrt = SqrtPriceX96::from(price);

        // If the AMM price is decreasing, it is because the AMM is accepting T0 from
        // the contract.  An order that purchases T0 from the contract is a bid
        let is_bid = start_sqrt >= end_sqrt;

        // let direction = Direction::from_is_bid(is_bid);

        let Ok(res) = PoolPriceVec::from_price_range(start_price, end_price) else {
            return Default::default();
        };
        // let Ok(res) = PoolPriceVec::swap_to_price(start_price.clone(), end_sqrt,
        // direction) else {     return Default::default();
        // };

        trace!(?start_sqrt, ?end_sqrt, ?price, res.d_t0, res.d_t1, is_bid, "AMM swap calc");
        if is_bid {
            // if the amm is swapping from zero to one, it means that we need more liquidity
            // it in token 1 and less in token zero
            (I256::try_from(res.d_t0).unwrap() * I256::MINUS_ONE, I256::try_from(res.d_t1).unwrap())
        } else {
            // if we are one for zero, means we are adding liquidity in t0 and removing in
            // t1
            (I256::try_from(res.d_t0).unwrap(), I256::try_from(res.d_t1).unwrap() * I256::MINUS_ONE)
        }
    }

    fn fetch_amount_out_amount_in_non_partials(&self, price: Ray) -> (I256, I256) {
        let mut t0_delta = I256::ZERO;
        let mut t1_delta = I256::ZERO;
        self.book
            .asks()
            .iter()
            .filter(|ask| price >= ask.pre_fee_price(self.fee) && !ask.is_partial())
            .for_each(|ask| {
                let (q_in, q_out) = Self::get_amount_in_out(ask, ask.amount(), self.fee, price);
                trace!(q_in, q_out, "Nonpartial ask - full fill");
                // given its a ask, q_in is always t0
                t0_delta += I256::try_from(q_in).unwrap();
                t1_delta -= I256::try_from(q_out).unwrap();
            });

        self.book
            .bids()
            .iter()
            .filter(|bid| {
                price <= bid.pre_fee_price(self.fee).inv_ray_round(false) && !bid.is_partial()
            })
            .for_each(|bid| {
                let (q_in, q_out) = Self::get_amount_in_out(bid, bid.amount(), self.fee, price);
                trace!(q_in, q_out, "Nonpartial bid - full fill");
                // given its a bid, q_in is always t1
                t0_delta -= I256::try_from(q_out).unwrap();
                t1_delta += I256::try_from(q_in).unwrap();
            });

        (t0_delta, t1_delta)
    }

    /// returns t0 delta, t1 delta, optional t0 extra fill and optional t1 extra
    /// fill.
    /// this favours partially filling
    fn fetch_amount_in_amount_out_partials(
        &self,
        price: Ray
    ) -> (I256, I256, u128, u128, u128, u128) {
        let mut t0_delta = I256::ZERO;
        let mut t1_delta = I256::ZERO;

        let mut ask_optional_t0: u128 = 0;
        let mut ask_optional_t1: u128 = 0;
        let mut bid_optional_t0: u128 = 0;
        let mut bid_optional_t1: u128 = 0;

        // I can make this all cleaner but not yet
        // self.book
        //     .all_orders_iter()
        //     // Filter for only valid partial orders
        //     .filter(|o| {
        //         o.is_partial()
        //             && if o.is_bid {
        //                 price <= o.pre_fee_price(self.fee).inv_ray_round(false)
        //             } else {
        //                 price >= o.pre_fee_price(self.fee)
        //             }
        //     })
        //     .for_each(|o| {});

        self.book
            .asks()
            .iter()
            .filter(|ask| price >= ask.pre_fee_price(self.fee) && ask.is_partial())
            .for_each(|ask| {
                if price == ask.price() {
                    // min prices
                    let (q_in_min, q_out_min) =
                        Self::get_amount_in_out(ask, ask.min_amount(), self.fee, price);
                    trace!(q_in_min, q_out_min, "Partial ask - min fill");
                    t0_delta += I256::try_from(q_in_min).unwrap();
                    t1_delta -= I256::try_from(q_out_min).unwrap();

                    // max amount
                    let (q_in_max, q_out_max) =
                        Self::get_amount_in_out(ask, ask.amount(), self.fee, price);
                    // if we are asks then
                    ask_optional_t0 += q_in_max - q_in_min;
                    ask_optional_t1 += q_out_max - q_out_min;

                    return;
                }

                // max amount
                let (q_in, q_out) = Self::get_amount_in_out(ask, ask.amount(), self.fee, price);
                trace!(q_in, q_out, "Partial ask - full fill");
                // given its a ask, q_in is always t0
                t0_delta += I256::try_from(q_in).unwrap();
                t1_delta -= I256::try_from(q_out).unwrap();
            });

        self.book
            .bids()
            .iter()
            .filter(|bid| {
                price <= bid.pre_fee_price(self.fee).inv_ray_round(false) && bid.is_partial()
            })
            .for_each(|bid| {
                // favour filling asks for now. will come back and fix later
                if price == bid.price().inv_ray_round(true) {
                    let (q_in_min, q_out_min) =
                        Self::get_amount_in_out(bid, bid.min_amount(), self.fee, price);
                    trace!(q_in_min, q_out_min, "Partial bid - min fill");

                    t0_delta -= I256::try_from(q_out_min).unwrap();
                    t1_delta += I256::try_from(q_in_min).unwrap();

                    let (q_in_max, q_out_max) =
                        Self::get_amount_in_out(bid, bid.amount(), self.fee, price);
                    bid_optional_t0 += q_out_max - q_out_min;
                    bid_optional_t1 += q_in_max - q_in_min;
                    return;
                }

                let (q_in, q_out) = Self::get_amount_in_out(bid, bid.amount(), self.fee, price);
                trace!(q_in, q_out, "Partial bid - full fill");
                // given its a bid, q_in is always t1
                t0_delta -= I256::try_from(q_out).unwrap();
                t1_delta += I256::try_from(q_in).unwrap();
            });

        (t0_delta, t1_delta, ask_optional_t0, ask_optional_t1, bid_optional_t0, bid_optional_t1)
    }

    fn check_ucp(&self, price: Ray) -> SupplyDemandResult {
        let (book_t0, book_t1) = self.fetch_concentrated_liquidity(price);
        let (normal_t0, normal_t1) = self.fetch_amount_out_amount_in_non_partials(price);
        let (
            partial_t0,
            partial_t1,
            ask_optional_t0,
            ask_optional_t1,
            bid_optional_t0,
            bid_optional_t1
        ) = self.fetch_amount_in_amount_out_partials(price);

        let t0_sum = book_t0 + normal_t0 + partial_t0;
        let t1_sum = book_t1 + normal_t1 + partial_t1;

        tracing::trace!(
            self.solve_for_t0,
            ?price,
            ?book_t0,
            ?book_t1,
            ?normal_t0,
            ?normal_t1,
            ?partial_t0,
            ?partial_t1,
            ?t0_sum,
            ?t1_sum,
            ask_optional_t0,
            ask_optional_t1,
            bid_optional_t0,
            bid_optional_t1,
            "Testing price"
        );

        if t0_sum.is_zero() && t1_sum.is_zero() {
            return SupplyDemandResult::NaturallyEqual;
        }

        // Depending on how we're solving, we want to look at a specific excess
        // liquidity
        let excess_liquidity = if self.solve_for_t0 { &t0_sum } else { &t1_sum };

        // See if we have any partial amount available that actually drains liquidity
        let (available_drain, available_add) = if self.solve_for_t0 {
            (bid_optional_t0, ask_optional_t0)
        } else {
            (ask_optional_t1, bid_optional_t1)
        };

        // We need our absolute excess in all cases here
        let abs_excess = excess_liquidity.unsigned_abs().to::<u128>();
        // Option<(bid_partial_fill, ask_partial_fill)
        let (bid_fill_q, ask_fill_q, reward_t0) = if excess_liquidity.is_negative() {
            // If our available fill is not enough to resolve our excess liquidity, we can
            // end here
            let Some(remaining_add) = available_add.checked_sub(abs_excess) else {
                return SupplyDemandResult::MoreDemand;
            };
            // Otherwise let's see if we can fill any extra after this
            let additional_fillable = min(remaining_add, available_drain);
            if self.solve_for_t0 {
                // If I'm solving for T0, asks are providing me the extra T0 I need and bids are
                // matched with my additional_fillable.

                // For T0 solve we do not expect to have excess T0 to donate so `reward_t0` is
                // just zero.

                (price.quantity(additional_fillable, false), abs_excess + additional_fillable, 0)
            } else {
                // For T1 swap, we want to figure out how much excess T0 we have, which will
                // become our `reward_t0`.  We can assume that we will be swapping all of our
                // excess T1 (`abs_excess`) for T0 and that will be the amount of extra T0 we
                // have to give as a reward. `additional_fillable` is based on orders that have
                // been matched against each other, so its effect on our net balances should be
                // zero and it is allocated to partial orders on both sides of the book.

                // `abs_excess` is in T1 and on this path it is an underflow (negative
                // quantity), we will be getting our additional T1 from bid orders.  We should
                // also have an excess of T0 at this point that we're selling to those bid
                // orders, so we need to calculate how much of our T0 excess will be leaving
                // before we allocate the rest to rewards.  We would like to overestimate this
                // sum if possible to make sure we never have rounding errors.

                // We know that each bid order will round its output down, so if we convert
                // `abs_excess` to T0 at our UCP, we can also round down.  We can presume that
                // there will be at least 1 bid order accepting the T0 (also rounding down),
                // and if there is more than one order we will get multiple round-downs which
                // will mean that the actual output can only be lower than this estimate and we
                // can only successfully overestimate.

                // This is our overestimation of how much T0 we are sending out to bids
                let excess_t0_cost = price.inv_ray().inverse_quantity(abs_excess, false);
                // If we already have a surplus of T0 in the balance, find out how much we will
                // have left after we settle the bids providing our excess T1.  That's our
                // reward quantity. (If `t0_sum` is already negative we are in a bad place
                // overall and surely have nothing to donate to rewards)
                let reward_t0 = if t0_sum.is_positive() {
                    t0_sum
                        .unsigned_abs()
                        .to::<u128>()
                        .saturating_sub(excess_t0_cost)
                } else {
                    0_u128
                };

                (
                    // Bids are providing the extra T1 I need, and we add the amount of T1 our
                    // partials matched as `additional_fillable`
                    abs_excess + additional_fillable,
                    // Asks are only needed to provide the reciprocal match for
                    // `additional_fillable` (flipped because they are exact_in in T0)
                    price.inverse_quantity(additional_fillable, false),
                    // And our rewards
                    reward_t0
                )
            }
        } else {
            let Some(remaining_drain) = available_drain.checked_sub(abs_excess) else {
                return SupplyDemandResult::MoreSupply;
            };
            let additional_drainable = min(remaining_drain, available_add);
            if self.solve_for_t0 {
                // If I'm solving for T0, bids are draining my excess T0 and asks are matched
                // with my additional_fillable

                // For T0 solve we do not expect to have excess T0 to donate so `reward_t0` is
                // just zero.
                (
                    price.quantity(abs_excess + additional_drainable, false),
                    additional_drainable,
                    0_u128
                )
            } else {
                // This is very similar to above but we're going to logic it through in the
                // opposite direction.

                // `abs_excess` is in T1 and on this path it is an overflow (positive
                // quantity), we will be draining our excess T1 using ask orders.  We should
                // also have an underflow of T0 at this point that we're buying from those ask
                // orders, so we need to calculate how much we will overflow our T0 defecit to
                // know how much we can allocate to rewards.  We would like to underestimate
                // this sum if possible to make sure we never have rounding errors.

                // We know that each ask order will round its input up, so if we convert
                // `abs_excess` to T0 at our UCP, we can also round up.  We can presume that
                // there will be at least 1 ask order providing the T0 (also rounding up),
                // and if there is more than one order we will get multiple round-ups which
                // will mean that the actual input can only be higher than this estimate and we
                // can only successfully underestimate.

                // This is our underestimation of how much T0 we are getting in from asks
                let excess_t0_gain = price.inverse_quantity(abs_excess, true);
                // We should already have a defecit of T0 in the balance and this sale should
                // bring it positive.  If `t0_sum` is already positive...that's weird, but we
                // can still do this math.  So we see where we stand after our gain and donate
                // whatever positive value we have.
                let final_t0 = t0_sum + I256::unchecked_from(excess_t0_gain);
                let reward_t0 = if final_t0.is_positive() {
                    final_t0.unsigned_abs().to::<u128>()
                } else {
                    0_u128
                };

                // Bids will round up so if we round-down for bids we will only have extra in
                // For asks we already know how much new T0 we're getting in with no change
                (
                    // Bids are only needed to provide the reciprocal match for
                    // `additional_fillable` (not flipped because they are exact_in in T1)
                    additional_drainable,
                    // Asks are draining the extra T1 I have, and we add the amount of T1 our
                    // partials matched as `additional_fillable`.  We need to flip this because
                    // asks are exact_in in T0.
                    price.inverse_quantity(abs_excess + additional_drainable, false),
                    // And our rewards
                    reward_t0
                )
            }
        };

        SupplyDemandResult::PartialFillEq { bid_fill_q, ask_fill_q, reward_t0 }
    }

    /// calculates given the supply, demand, optional supply and optional demand
    /// what way the algo's price should move if we want it too
    fn get_amount_in_out(
        order: &OrderWithStorageData<GroupedVanillaOrder>,
        fill_amount: u128,
        fee: u128,
        ray_ucp: Ray
    ) -> (u128, u128) {
        let is_bid = order.is_bid();
        let exact_in = order.exact_in();
        let gas = order.priority_data.gas.to::<u128>();
        let (t1, t0_net, t0_fee) =
            get_quantities_at_price(is_bid, exact_in, fill_amount, gas, fee, ray_ucp);

        // If our order is a bid, our T1 entirely enters the market for liquidity but we
        // have to consume t0_net, t0_fee and gas from the market as we convert the
        // incoming T1 into T0.  For asks, because our fee and gas are taken from the
        // incoming T0, only t0_net enters the market as liquidity.  The entire t1
        // quantity exits.
        if is_bid { (t1, t0_net + t0_fee + gas) } else { (t0_net, t1) }
    }

    /// helper functions for grabbing all orders that we filled at ucp
    fn fetch_orders_at_ucp(&self, fetch: &UcpSolution) -> Vec<OrderOutcome> {
        let (mut bid_partial, mut ask_partial) = fetch.partial_fills.unwrap_or_default();

        self.book
            .all_orders_iter()
            .map(|o| {
                let outcome = match (o.price_t1_over_t0().cmp(&fetch.ucp), o.is_bid) {
                    // A bid with a higher price than UCP or an ask with a lower price than UCP is
                    // filled
                    (Ordering::Greater, true) | (Ordering::Less, false) => {
                        trace!("Order completely filled due to price position");
                        OrderFillState::CompleteFill
                    }
                    // A bid with a lower price than UCP or an ask with a higher price than UCP is
                    // unfilled
                    (Ordering::Greater, false) | (Ordering::Less, true) => {
                        trace!("Order unfilled due to price position");
                        OrderFillState::Unfilled
                    }
                    // At the precise price, exact orders are all filled and partial orders MIGHT be
                    // filled more than the minimum
                    (Ordering::Equal, _) => {
                        if o.is_partial() {
                            let partial_q =
                                if o.is_bid { &mut bid_partial } else { &mut ask_partial };
                            if *partial_q > 0 {
                                let max_partial = o.max_q() - o.min_amount();
                                let res = if *partial_q > max_partial {
                                    trace!(
                                        o.is_bid,
                                        partial_q,
                                        max_partial,
                                        "Partial order completely filled at UCP"
                                    );
                                    OrderFillState::CompleteFill
                                } else {
                                    trace!(
                                        o.is_bid,
                                        partial_q, "Partial order partially filled at UCP"
                                    );
                                    OrderFillState::PartialFill(o.min_amount() + *partial_q)
                                };
                                *partial_q = partial_q.saturating_sub(max_partial);
                                res
                            } else {
                                // If we have no partial, it's filled for the minimum
                                trace!("Partial order minimally filled at UCP due to no partial_q");
                                OrderFillState::PartialFill(o.min_amount())
                            }
                        } else {
                            // Non-partial orders at the target price were completely filled
                            trace!("Non-partial order completely filled at UCP");
                            OrderFillState::CompleteFill
                        }
                    }
                };
                OrderOutcome { id: o.order_id, outcome }
            })
            .collect::<Vec<_>>()
    }

    /// Return the NetAmmOrder that moves the AMM to our UCP
    fn fetch_amm_movement_at_ucp(&self, ucp: Ray) -> Option<NetAmmOrder> {
        let start_price = self.amm_start_price.clone()?;
        let end_price = start_price
            .snapshot()
            .at_price(SqrtPriceX96::from(ucp))
            .ok()?;

        let Ok(res) = PoolPriceVec::from_price_range(start_price, end_price) else {
            tracing::error!("Unable to create AMM movement at UCP");
            return None;
        };

        let mut tob_amm = NetAmmOrder::new(Direction::from_is_bid(res.zero_for_one()));
        tob_amm.add_quantity(res.d_t0, res.d_t1);

        Some(tob_amm)
    }

    // short on asks.
    pub fn solution(
        &mut self,
        searcher: Option<OrderWithStorageData<TopOfBlockOrder>>
    ) -> PoolSolution {
        let Some(mut price_and_partial_solution) = self.solve_clearing_price() else {
            return PoolSolution {
                id: self.book.id(),
                searcher,
                limit: self
                    .book
                    .all_orders_iter()
                    .map(|o| OrderOutcome {
                        id:      o.order_id,
                        outcome: OrderFillState::Unfilled
                    })
                    .collect(),
                ..Default::default()
            };
        };

        let limit = self.fetch_orders_at_ucp(&price_and_partial_solution);
        let mut amm = self.fetch_amm_movement_at_ucp(price_and_partial_solution.ucp);

        // get weird overflow values
        if limit.is_empty() {
            price_and_partial_solution.ucp = Ray::default();
            amm = None;
        }

        PoolSolution {
            id: self.book.id(),
            ucp: price_and_partial_solution.ucp,
            amm_quantity: amm,
            limit,
            searcher,
            reward_t0: price_and_partial_solution.reward_t0
        }
    }

    fn solve_clearing_price(&self) -> Option<UcpSolution> {
        let ep = Ray::from(U256::from(1));
        let mut p_max = Ray::from(self.book.highest_clearing_price().saturating_add(*ep));
        let mut p_min = Ray::from(self.book.lowest_clearing_price().saturating_sub(*ep));

        let two = U256::from(2);
        while (p_max - p_min) > ep {
            // grab all supply and demand
            let p_mid = (p_max + p_min) / two;

            // the delta of t0
            let res = self.check_ucp(p_mid);

            match (res, self.solve_for_t0) {
                // If there's too much supply of T0 or too much demand of T1, we want to look at a
                // lower price
                (SupplyDemandResult::MoreSupply, true)
                | (SupplyDemandResult::MoreDemand, false) => p_max = p_mid,
                // If there's too much supply of T1 or too much demand for T0, we want to look at a
                // higher price
                (SupplyDemandResult::MoreSupply, false)
                | (SupplyDemandResult::MoreDemand, true) => p_min = p_mid,
                (SupplyDemandResult::NaturallyEqual, _) => {
                    println!("solved based on sup, demand no partials");

                    return Some(UcpSolution {
                        ucp:           p_mid,
                        partial_fills: None,
                        reward_t0:     0_u128
                    });
                }
                (SupplyDemandResult::PartialFillEq { bid_fill_q, ask_fill_q, reward_t0 }, _) => {
                    println!("solved based on sup, demand with partial order");
                    return Some(UcpSolution {
                        ucp: p_mid,
                        partial_fills: Some((bid_fill_q, ask_fill_q)),
                        reward_t0
                    });
                }
            }
        }

        None
    }
}

#[derive(Debug)]
struct UcpSolution {
    /// Solved uniform clearing price in T1/T0 format
    ucp:           Ray,
    /// Partial fill quantities in format `Option<(bid_partial, ask_partial)>`
    partial_fills: Option<(u128, u128)>,
    /// Extra T0 that should be added to rewards
    reward_t0:     u128
}

#[derive(Debug)]
pub enum SupplyDemandResult {
    MoreSupply,
    MoreDemand,
    NaturallyEqual,
    PartialFillEq { bid_fill_q: u128, ask_fill_q: u128, reward_t0: u128 }
}
