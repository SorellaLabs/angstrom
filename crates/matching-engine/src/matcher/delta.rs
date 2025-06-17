use std::{
    cmp::{Ordering, Reverse, min},
    collections::HashSet
};

use alloy_primitives::{I256, Sign, U256};
use angstrom_types::{
    contract_payloads::angstrom::TopOfBlockOrder as ContractTopOfBlockOrder,
    matching::{
        SqrtPriceX96, get_quantities_at_price,
        uniswap::{Direction, Quantity}
    },
    orders::{NetAmmOrder, OrderFillState, OrderId, OrderOutcome, PoolSolution},
    sol_bindings::{
        RawPoolOrder, Ray,
        grouped_orders::{AllOrders, OrderWithStorageData},
        rpc_orders::TopOfBlockOrder
    },
    uni_structure::pool_swap::PoolSwapResult
};
use base64::Engine;
use itertools::Itertools;
use rand_distr::num_traits::Zero;
use serde::{Deserialize, Serialize};
use tracing::{Level, debug, trace};
use uniswap_v3_math::tick_math::{MAX_SQRT_RATIO, MIN_SQRT_RATIO};
use uniswap_v4::uniswap::pool::U256_1;

use crate::OrderBook;

struct OrderLiquidity {
    net_t0:          I256,
    net_t1:          I256,
    min_at_ucp_t1:   I256,
    last_mile_solve: HashSet<OrderId>
}

/// Enum describing what kind of ToB order we want to use to set the initial AMM
/// price for our DeltaMatcher
#[derive(Clone, Debug, Serialize, Deserialize)]
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
    book:               &'a OrderBook,
    fee:                u128,
    /// If true, we solve for T0.  If false we solve for T1.
    solve_for_t0:       bool,
    /// changes if there is a tob or not
    amm_start_location: Option<PoolSwapResult<'a>>
}

impl<'a> DeltaMatcher<'a> {
    pub fn new(book: &'a OrderBook, tob: DeltaMatcherToB, solve_for_t0: bool) -> Self {
        // Dump if matcher dumps are enabled
        if tracing::event_enabled!(target: "dump::delta_matcher", Level::TRACE) {
            // Dump the solution
            let json = serde_json::to_string(&(book, tob.clone(), solve_for_t0)).unwrap();
            let b64_output = base64::prelude::BASE64_STANDARD.encode(json.as_bytes());
            trace!(target: "dump::delta_matcher", data = b64_output, "Raw DeltaMatcher data");
        }

        let fee = book.amm().map(|amm| amm.fee()).unwrap_or_default() as u128;
        let amm_start_location = match tob {
            // If we have an order, apply that to the AMM start price
            DeltaMatcherToB::Order(ref tob) => book.amm().map(|snapshot| {
                ContractTopOfBlockOrder::calc_vec_and_reward(tob, snapshot)
                    .expect("Order structure should be valid and never fail")
                    .0
            }),
            // If we have a fixed shift, apply that to the AMM start price (Not yet operational)
            DeltaMatcherToB::FixedShift(..) => panic!("not implemented"),
            // If we have no order or shift, we just use the AMM start price as-is
            DeltaMatcherToB::None => book.amm().map(|book| book.noop())
        };

        Self { book, amm_start_location, fee, solve_for_t0 }
    }

    /// panics if there is no amm swap
    pub fn try_get_amm_location(&self) -> &PoolSwapResult<'_> {
        self.amm_start_location.as_ref().unwrap()
    }

    fn process_swap(&self, is_bid: bool, res: PoolSwapResult<'_>) -> (I256, I256) {
        if is_bid {
            // if the amm is swapping from zero to one, it means that we need more liquidity
            // it in token 1 and less in token zero
            (
                I256::try_from(res.total_d_t0).unwrap() * I256::MINUS_ONE,
                I256::try_from(res.total_d_t1).unwrap()
            )
        } else {
            // if we are one for zero, means we are adding liquidity in t0 and removing in
            // t1
            (
                I256::try_from(res.total_d_t0).unwrap(),
                I256::try_from(res.total_d_t1).unwrap() * I256::MINUS_ONE
            )
        }
    }

    fn fetch_concentrated_liquidity(&self, price: Ray) -> (I256, I256) {
        let end_sqrt = if price.within_sqrt_price_bounds() {
            SqrtPriceX96::from(price)
        } else {
            let this_price: SqrtPriceX96 = MIN_SQRT_RATIO.into();
            let ray: Ray = this_price.into();

            if price <= ray { this_price } else { MAX_SQRT_RATIO.into() }
        };

        let Some(pool) = self.amm_start_location.as_ref() else { return Default::default() };

        let start_sqrt = pool.end_price;

        // If the AMM price is decreasing, it is because the AMM is accepting T0 from
        // the contract.  An order that purchases T0 from the contract is a bid
        let is_bid = start_sqrt >= end_sqrt;

        // swap to start
        let Ok(res) = pool.swap_to_price(Direction::from_is_bid(is_bid), end_sqrt) else {
            return Default::default();
        };

        self.process_swap(is_bid, res)
    }

    /// Combined method that finds total order liquidity available at a price.
    /// This operates off of a few assumptions listed below:
    ///
    /// - If the target price is less beneficial than the order's limit price,
    ///   the order should be excluded
    /// - If the target price is more beneficial than the order's limit price,
    ///   the order should be filled completely
    /// - If the target price is equal to the order's limit price, then we will
    ///   first attempt to fill partial orders as completely as possible and
    ///   then after all partial orders are filled we will fill as many exact
    ///   orders as we can
    ///
    /// While this method is not specifically responsible for filling orders,
    /// the data we gather here and the values we choose for minimum required
    /// fill and maximum possible fill/slack are designed to support this
    /// matching technique
    fn order_liquidity(&self, price: Ray, perma_killed: &HashSet<OrderId>) -> OrderLiquidity {
        let mut net_t0 = I256::ZERO;
        let mut net_t1 = I256::ZERO;
        let mut min_t1_ucp = I256::ZERO;

        let mut last_mile_solve = HashSet::new();

        // Iterate over all the orders valid at this price
        self.book
            .bids()
            .iter()
            .filter(|o| price <= o.pre_fee_price(self.fee).inv_ray_round(false))
            .chain(
                self.book
                    .asks()
                    .iter()
                    .filter(|o| price >= o.pre_fee_price(self.fee))
            )
            .filter(|o| perma_killed.contains(&o.order_id))
            .for_each(|o| {
                // If we're precisely at our target price, we determine what our minimum and
                // maximum output is for this order.  Otherwise our minimum is "all of it"
                let at_price = price == o.price_t1_over_t0();
                let min_q = o.amount();

                if at_price {
                    last_mile_solve.insert(o.order_id);
                    let (min_in, min_out) = Self::get_amount_in_out(o, min_q, self.fee, price);

                    if o.is_bid {
                        min_t1_ucp += min_in
                    } else {
                        min_t1_ucp += I256::unchecked_from(min_out).saturating_neg()
                    }

                    return;
                };

                // Calculate and account for our minimum fill, preserving quantity numbers in
                // case we need to use them for slack later

                let (min_in, min_out) = Self::get_amount_in_out(o, min_q, self.fee, price);

                // Add the mandatory portion of this order to our overall delta
                let s_in = I256::try_from(min_in).unwrap();
                let s_out = I256::try_from(min_out).unwrap();
                let (t0_d, t1_d) = if o.is_bid {
                    // For bid, output is negative T0, input is positive T1
                    (s_out.saturating_neg(), s_in)
                } else {
                    // For an ask, input is positive T0, output is negative T1
                    (s_in, s_out.saturating_neg())
                };
                net_t0 += t0_d;
                net_t1 += t1_d;

                // If we have a maximum available amount, put that into our slack to be matched
                // later
                trace!(at_price, is_bid = o.is_bid, ?t0_d, ?t1_d, "Processed order");
            });

        OrderLiquidity { net_t0, net_t1, last_mile_solve, min_at_ucp_t1: min_t1_ucp }
    }

    fn check_killable_orders(
        killable_orders: &[(OrderId, u128, bool)],
        is_bid: bool,
        min_target: u128
    ) -> (u128, Option<Vec<OrderId>>) {
        let (total_elim, order_ids) = killable_orders
            .iter()
            .filter(|x| x.2 == is_bid)
            .sorted_by_key(|x| Reverse(x.1))
            .fold_while::<(u128, Option<Vec<OrderId>>), _>(
                (0_u128, None),
                |(total_elim, mut elim_ids), (order_id, order_q, _)| {
                    let elim_vec = elim_ids.get_or_insert_default();
                    elim_vec.push(*order_id);
                    let new_elim = total_elim + *order_q;
                    let new_acc = (new_elim, elim_ids);
                    if new_elim > min_target {
                        itertools::FoldWhile::Done(new_acc)
                    } else {
                        itertools::FoldWhile::Continue(new_acc)
                    }
                }
            )
            .into_inner();
        // We only suggest killing orders if we can actually succeed at fixing this
        // price
        if total_elim >= min_target { (total_elim, order_ids) } else { (0_u128, None) }
    }

    /// NOTE: only implied for solve for t1
    fn solve_last_mile(
        &self,
        price: Ray,
        liq: OrderLiquidity,
        amm: (I256, I256)
    ) -> Option<SupplyDemandResult> {
        let (amm_t0, amm_t1) = amm;
        let OrderLiquidity { net_t0, net_t1, mut last_mile_solve, .. } = liq;
        let t1_sum = amm_t1 + *net_t1;
        let t0_sum = amm_t0 + *net_t0;

        let current_solve = None;

        // we might be naturally equal but if we can solve more volume with partials,
        // than this solution is objectively better
        if t1_sum.is_zero() {
            // any remainder t0 should be donated.
            let t0_reward = t0_sum
                .is_positive()
                .then(|| t0_sum.to::<u128>())
                .unwrap_or_default();

            current_solve = Some(SupplyDemandResult::NaturallyEqual(last_mile_solve, t0_reward));
        }

        let t1_bid_slack = 0u128;
        let t1_ask_slack = 0u128;

        for order in itertools::kmerge_by(
            vec![
                self.book
                    .bids()
                    .into_iter()
                    .filter(|f| last_mile_solve.contains(&f.order_id)),
                self.book
                    .asks()
                    .into_iter()
                    .filter(|f| last_mile_solve.contains(&f.order_id)),
            ],
            |a, b| a.priority_data.volume.lt(&b.priority_data.volume)
        ) {
            // we are processing and thus needs to be removed.
            last_mile_solve.remove(&order.order_id);

            if order.is_bid {
                let (min_in, min_out) =
                    Self::get_amount_in_out(order, order.min_amount(), self.fee, price);
                let (max_in, _) = Self::get_amount_in_out(order, order.amount(), self.fee, price);
                let delta_v = (min_in != max_in)
                    .then_some(|| max_in - min_in)
                    .unwrap_or_default();

                t1_bid_slack += delta_v;
                t1_sum += min_in;
                t0_sum += I256::unchecked_from(min_out).saturating_neg();
            } else {
                let (min_in, min_out) =
                    Self::get_amount_in_out(order, order.min_amount(), self.fee, price);
                let (_, max_out) = Self::get_amount_in_out(order, order.amount(), self.fee, price);

                let delta_v = (min_out != max_out)
                    .then_some(|| max_out - min_out)
                    .unwrap_or_default();

                t1_ask_slack += delta_v;
                t1_sum += I256::unchecked_from(min_out).saturating_neg();
                t0_sum += min_in;
            }

            // The first thing we try to do is see if we can solve with all the orders and
            // compose with partial.

            // The logic gets messy here
            if t1_sum.is_zero() {
                let t0_reward = t0_sum
                    .is_positive()
                    .then(|| t0_sum.to::<u128>())
                    .unwrap_or_default();

                current_solve =
                    Some(SupplyDemandResult::NaturallyEqual(last_mile_solve, t0_reward));
            } else if t1_sum.is_positive() {
                // if we are positive, we want to see if we can add some amount of ask-slack to
                // make this zero
                let delta = t1_sum - I256::unchecked_from(t1_ask_slack);

                // we can solve here
                if delta.is_negative() || delta.is_zero() {
                    let rem = delta.abs().to::<u128>();

                    // we solve with only a full fill on 1 side
                    let (ask_q, bid_q) = if rem.is_zero() {
                        (t1_ask_slack, 0)
                    } else {
                        if rem > t1_bid_slack {
                            // If our remainder is more than our total bid-slack,
                            // we can full fill this with our remainder t1_bid_slack
                            (t1_sum.to::<u128>() + t1_bid_slack, t1_bid_slack)
                        } else {
                            // If we have less remainder than our bid_slack,
                            // we can fill for full remainder amount
                            (t1_sum.to::<u128>() + rem, rem)
                        }
                    };

                    let abs_excess = t1_sum.abs().to::<u128>();
                    let excess_t0_gain = price.inverse_quantity(abs_excess, true);
                    let final_t0 = t0_sum + I256::unchecked_from(excess_t0_gain);

                    let reward_t0 = if final_t0.is_positive() {
                        final_t0.unsigned_abs().to::<u128>()
                    } else {
                        0_u128
                    };

                    current_solve = Some(SupplyDemandResult::PartialFillEq {
                        bid_fill_q: bid_q,
                        ask_fill_q: ask_q,
                        reward_t0,
                        killed: last_mile_solve.clone()
                    });

                    continue;
                }
            } else {
                // Case were we are negative.
                let delta = t1_sum + I256::unchecked_from(t1_bid_slack);

                if delta.is_positive() || delta.is_zero() {
                    let rem = delta.to::<u128>();

                    // we solve with only a full fill on 1 side
                    let (ask_q, bid_q) = if rem.is_zero() {
                        (0, t1_bid_slack)
                    } else {
                        if rem > t1_ask_slack {
                            // if we have more remainder than we can subtract, we subtract full
                            // slack
                            (t1_ask_slack, t1_sum.abs().to::<u128>() + t1_ask_slack)
                        } else {
                            // If we have more ask slack, we put in full remainder
                            (rem, t1_sum.abs().to::<u128>() + rem)
                        }
                    };

                    let abs_excess = t1_sum.abs().to::<u128>();
                    let excess_t0_cost = price.inv_ray().inverse_quantity(abs_excess, false);
                    let reward_t0 = if t0_sum.is_positive() {
                        t0_sum
                            .unsigned_abs()
                            .to::<u128>()
                            .saturating_sub(excess_t0_cost)
                    } else {
                        0_u128
                    };

                    current_solve = Some(SupplyDemandResult::PartialFillEq {
                        bid_fill_q: bid_q,
                        ask_fill_q: ask_q,
                        reward_t0,
                        killed: last_mile_solve.clone()
                    });

                    continue;
                }
            }
        }

        current_solve
    }

    fn check_ucp(
        &self,
        price: Ray,
        perma_killed: &HashSet<OrderId>,
        try_kill: bool
    ) -> (CheckUcpStats, SupplyDemandResult) {
        let amm = self.fetch_concentrated_liquidity(price);

        let liq = self.order_liquidity(price, perma_killed);
        let t1_sum = amm.1 + liq.net_t1;
        let t1_sum_with_end_min = t1_sum + liq.min_at_ucp_t1;
        let all_current_killable = liq.last_mile_solve.clone();

        let stats = CheckUcpStats {
            amm_t0:   amm.0,
            amm_t1:   amm.1,
            order_t0: liq.net_t0,
            order_t1: liq.net_t1
        };

        // If we have a solve on the last mile.
        if let Some(res) = self.solve_last_mile(price, liq, amm) {
            return (stats, res);
        }

        let kills = try_kill.then(||
            // we can look at just the min here as we know that if there was a intersection with
            // the positive, we would of had a fill via last mile solve.
            if t1_sum_with_end_min.is_positive() {
                // remove bids.
                 self.calculate_kills(price, true, all_current_killable, t1_sum_with_end_min.to())
            } else {
                // remove asks
                 self.calculate_kills(price, false, all_current_killable, t1_sum_with_end_min.abs().to())
            }).flatten();

        if t1_sum.is_positive() {
            (stats, SupplyDemandResult::MoreSupply(kills))
        } else {
            (stats, SupplyDemandResult::MoreDemand(kills))
        }
    }

    /// given the current ucp, what orders
    fn calculate_kills(
        &self,
        price: Ray,
        bids: bool,
        orders: HashSet<OrderId>,
        target_amount: u128
    ) -> Option<Vec<OrderId>> {
        let (total_killed, ids) = if bids {
            self.book
                .bids()
                .into_iter()
                .filter(|o| orders.contains(&o.order_id))
                .sorted_by(|a, b| b.priority_data.volume.cmp(&a.priority_data.volume))
                .fold_while::<(u128, Option<Vec<OrderId>>), _>(
                    (0_u128, None),
                    |(total_elim, mut elim_ids), order| {
                        let elim_vec = elim_ids.get_or_insert_default();
                        let (t1_bid, _) =
                            Self::get_amount_in_out(order, order.amount(), self.fee, price);

                        elim_vec.push(order.order_id);
                        let new_elim = total_elim + t1_bid;
                        let new_acc = (new_elim, elim_ids);
                        if new_elim > target_amount {
                            itertools::FoldWhile::Done(new_acc)
                        } else {
                            itertools::FoldWhile::Continue(new_acc)
                        }
                    }
                )
                .into_inner()
        } else {
            self.book
                .asks()
                .into_iter()
                .filter(|o| orders.contains(&o.order_id))
                .sorted_by(|a, b| b.priority_data.volume.cmp(&a.priority_data.volume))
                .fold_while::<(u128, Option<Vec<OrderId>>), _>(
                    (0_u128, None),
                    |(total_elim, mut elim_ids), order| {
                        let elim_vec = elim_ids.get_or_insert_default();
                        // if were a ask
                        let (_, t1_ask) =
                            Self::get_amount_in_out(order, order.amount(), self.fee, price);

                        elim_vec.push(order.order_id);
                        let new_elim = total_elim + t1_ask;
                        let new_acc = (new_elim, elim_ids);
                        if new_elim > target_amount {
                            itertools::FoldWhile::Done(new_acc)
                        } else {
                            itertools::FoldWhile::Continue(new_acc)
                        }
                    }
                )
                .into_inner()
        };

        (total_killed >= target_amount).then_some(ids).flatten()
    }

    /// calculates given the supply, demand, optional supply and optional demand
    /// what way the algo's price should move if we want it too
    fn get_amount_in_out(
        order: &OrderWithStorageData<AllOrders>,
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
        let (bid_partial, ask_partial) = fetch.partial_fills.unwrap_or_default();

        //Precompute total deltas for pro-rata calculation
        let mut total_bid_delta: u128 = 0;
        let mut total_ask_delta: u128 = 0;

        for o in self.book.all_orders_iter() {
            if fetch.killed.contains(&o.order_id) {
                continue;
            }
            match (o.price_t1_over_t0().cmp(&fetch.ucp), o.is_bid) {
                (Ordering::Equal, true) if o.is_partial() => {
                    let delta = o.amount().saturating_sub(o.min_amount());
                    total_bid_delta += delta;
                }
                (Ordering::Equal, false) if o.is_partial() => {
                    let delta = o.amount().saturating_sub(o.min_amount());
                    total_ask_delta += delta;
                }
                _ => {}
            }
        }

        self.book
            .all_orders_iter()
            .map(|o| {
                let outcome = if fetch.killed.contains(&o.order_id) {
                    OrderFillState::Killed
                } else {
                    match (o.price_t1_over_t0().cmp(&fetch.ucp), o.is_bid) {
                        // Fully filled orders
                        (Ordering::Greater, true) | (Ordering::Less, false) => {
                            OrderFillState::CompleteFill
                        }
                        // Fully unfilled orders
                        (Ordering::Greater, false) | (Ordering::Less, true) => {
                            OrderFillState::Unfilled
                        }
                        // Partial fill candidates at UCP
                        (Ordering::Equal, true) => {
                            if !o.is_partial() {
                                OrderFillState::CompleteFill
                            } else {
                                let delta = o.amount().saturating_sub(o.min_amount());
                                if delta == 0 || total_bid_delta == 0 {
                                    OrderFillState::PartialFill(o.min_amount())
                                } else {
                                    let portion =
                                        (delta as u128 * bid_partial as u128) / total_bid_delta;
                                    OrderFillState::PartialFill(o.min_amount() + portion)
                                }
                            }
                        }
                        (Ordering::Equal, false) => {
                            if !o.is_partial() {
                                OrderFillState::CompleteFill
                            } else {
                                let delta = o.amount().saturating_sub(o.min_amount());
                                if delta == 0 || total_ask_delta == 0 {
                                    OrderFillState::PartialFill(o.min_amount())
                                } else {
                                    let portion =
                                        (delta as u128 * ask_partial as u128) / total_ask_delta;
                                    OrderFillState::PartialFill(o.min_amount() + portion)
                                }
                            }
                        }
                    }
                };

                OrderOutcome { id: o.order_id, outcome }
            })
            .collect()
    }

    /// Return the NetAmmOrder that moves the AMM to our UCP
    fn fetch_amm_movement_at_ucp(&self, ucp: Ray) -> Option<NetAmmOrder> {
        let end_price_sqrt = SqrtPriceX96::from(ucp);
        let Some(pool) = self.amm_start_location.as_ref() else { return Default::default() };

        let is_bid = pool.end_price >= end_price_sqrt;
        let direction = Direction::from_is_bid(is_bid);

        let Ok(res) = pool.swap_to_price(direction, end_price_sqrt) else {
            return Default::default();
        };

        let mut tob_amm = NetAmmOrder::new(direction);
        tob_amm.add_quantity(res.total_d_t0, res.total_d_t1);

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
        if limit.iter().filter(|f| f.is_filled()).count() == 0 {
            price_and_partial_solution.ucp = Ray::default();
            amm = None;
        }

        PoolSolution {
            id: self.book.id(),
            ucp: price_and_partial_solution.ucp,
            amm_quantity: amm,
            limit,
            searcher,
            reward_t0: price_and_partial_solution.reward_t0,
            fee: self.fee as u32
        }
    }

    /// For all cases, because of the precision RAY offers, we will overshoot
    /// the amm a lot, this means that there is a solution with the amm were
    /// we don't solve because amm doesn't have the same precision as ray.
    /// There are also other cases were Ray itself doesn't have the
    /// precision. In these cases, this amount will always be less than 1 unit
    /// delta of SqrtPriceX96. because of this, we assume that any dusting
    /// within the bounds of 1 SqrtPriceX96 unit to be valid solution.
    fn fetch_lower_upper_liq_bounds(&self, price: Ray, stats: &CheckUcpStats) -> Option<U256> {
        let end_mid_sqrt = if price.within_sqrt_price_bounds() {
            SqrtPriceX96::from(price)
        } else {
            return None;
        };

        let end_upper_sqrt = end_mid_sqrt + SqrtPriceX96::from(U256_1);
        let end_lower_sqrt = end_mid_sqrt - SqrtPriceX96::from(U256_1);

        let Some(pool) = self.amm_start_location.as_ref() else { return Default::default() };
        let start_sqrt = pool.end_price;

        // Handle upper.
        let is_bid = start_sqrt >= end_upper_sqrt;
        let Ok(res) = pool.swap_to_price(Direction::from_is_bid(is_bid), end_upper_sqrt) else {
            return Default::default();
        };
        let (t0_upper, t1_upper) = self.process_swap(is_bid, res);

        // Handle lower.
        let is_bid = start_sqrt >= end_lower_sqrt;
        let Ok(res) = pool.swap_to_price(Direction::from_is_bid(is_bid), end_lower_sqrt) else {
            return Default::default();
        };
        let (t0_lower, t1_lower) = self.process_swap(is_bid, res);

        // we take the biggest 1 SqrtPriceX96 unit move and return this
        (if self.solve_for_t0 {
            let (t0_upper, t0, t0_lower) = (t0_upper.abs(), stats.amm_t0.abs(), t0_lower.abs());
            // we take the max of upper lower
            if t0_upper > t0 {
                Some((t0_upper - t0).max(t0 - t0_lower))
            } else {
                Some((t0 - t0_upper).max(t0_lower - t0))
            }
        } else {
            let (t1_upper, t1, t1_lower) = (t1_upper.abs(), stats.amm_t1.abs(), t1_lower.abs());
            if t1_upper > t1 {
                Some((t1_upper - t1).max(t1 - t1_lower))
            } else {
                Some((t1 - t1_upper).max(t1_lower - t1))
            }
        })
        .map(|int| int.into_raw())
    }

    fn try_solve_dust_solution(
        &self,
        stats: &CheckUcpStats,
        res: &SupplyDemandResult,
        p_mid: Ray,
        dust: Option<&(U256, UcpSolution)>,
        killed: &HashSet<OrderId>
    ) -> Option<(U256, UcpSolution)> {
        // while this works. Dusting should actually be bounding by the limitation of
        // the amm's SqrtPriceX96 precision.

        let (total_liq, price_for_one) = if self.solve_for_t0 {
            (stats.amm_t0 + stats.order_t0, self.fetch_lower_upper_liq_bounds(p_mid, stats)?)
        } else {
            (stats.amm_t1 + stats.order_t1, self.fetch_lower_upper_liq_bounds(p_mid, stats)?)
        };

        if let (Sign::Positive, e) = total_liq.into_sign_and_abs() {
            // Check to see if our excess is within one price unit
            if e <= price_for_one {
                trace!("Valid dust solution found");
                // We have a dust solution, let's store it
                let dust_q = dust.as_ref().map(|d| d.0).unwrap_or(U256::MAX);
                if e < dust_q {
                    let (partial_fills, reward_t0) = if let SupplyDemandResult::PartialFillEq {
                        bid_fill_q,
                        ask_fill_q,
                        reward_t0,
                        killed: _
                    } = res
                    {
                        (Some((*bid_fill_q, *ask_fill_q)), *reward_t0)
                    } else {
                        (None, 0_u128)
                    };
                    let solution = UcpSolution {
                        ucp: p_mid,
                        killed: killed.clone(),
                        partial_fills,
                        reward_t0
                    };

                    return Some((e, solution));
                }
            }
        }

        None
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn solve_clearing_price(&self) -> Option<UcpSolution> {
        // p_max is (highest bid || (MAX_PRICE - 1)) + 1
        let mut p_max = Ray::from(self.book.highest_clearing_price().saturating_add(U256_1));
        // p_min is (lowest ask || (MIN_PRICE + 1)) - 1
        let mut p_min = Ray::from(self.book.lowest_clearing_price().saturating_sub(U256_1));

        let two = U256::from(2);
        let four = U256::from(4);
        let mut perma_killed = HashSet::new();
        let mut dust: Option<(U256, UcpSolution)> = None;
        let mut valid_solution: Option<UcpSolution> = None;

        // Loop on a checked sub, if our prices ever overlap we'll terminate the loop
        while let Some(diff) = p_max.checked_sub(*p_min) {
            //Break if !((p_max - p_min) > 1)
            if diff <= U256_1 {
                break;
            }
            // We're willing to kill orders if and only if we're at the end of our
            // iteration.  I believe that a distance of four will capture the last 2 cycles
            // of iteration
            let can_perma_kill = diff <= four;
            // Find the midpoint that we'll be testing
            let p_mid = (p_max + p_min) / two;

            // the delta of t0
            let (stats, res) = {
                let check_ucp_span =
                    tracing::trace_span!("check_ucp", price = ?p_mid, can_perma_kill);
                check_ucp_span.in_scope(|| self.check_ucp(p_mid, &perma_killed, can_perma_kill))
            };

            // If we're on our last iterations, check to see if we can come up with a
            // "within_one" solution

            // Check to see if we've found a valid dust solution that is better than our
            // current dust solution if it exists.
            //
            // NOTE: dust solution wi
            if let Some(new_dust) =
                self.try_solve_dust_solution(&stats, &res, p_mid, dust.as_ref(), &perma_killed)
            {
                dust = Some(new_dust);
            }

            match (res, can_perma_kill, self.solve_for_t0) {
                (SupplyDemandResult::MoreSupply(Some(ko)), true, _)
                | (SupplyDemandResult::MoreDemand(Some(ko)), true, _) => {
                    tracing::info!(order_ids = ? ko, "Killing orders");
                    // Add our killed order to the set of orders we're skipping
                    perma_killed.extend(ko);
                    // Reset our price bounds so we start our match over
                    p_max = Ray::from(self.book.highest_clearing_price().saturating_add(U256_1));
                    p_min = Ray::from(self.book.lowest_clearing_price().saturating_sub(U256_1));
                }
                // If there's too much supply of T0 or too much demand of T1, we want to look at a
                // lower price
                (SupplyDemandResult::MoreSupply(_), _, true)
                | (SupplyDemandResult::MoreDemand(_), _, false) => p_max = p_mid,
                // If there's too much supply of T1 or too much demand for T0, we want to look at a
                // higher price
                (SupplyDemandResult::MoreSupply(_), _, false)
                | (SupplyDemandResult::MoreDemand(_), _, true) => p_min = p_mid,
                (SupplyDemandResult::NaturallyEqual(mut killed, reward), ..) => {
                    killed.extend(perma_killed);
                    debug!(
                        ucp = ?p_mid,
                        partial = false,
                        reward_t0 = 0_u128,
                        ?stats.amm_t0,
                        ?stats.amm_t1,
                        ?stats.order_t0,
                        ?stats.order_t1,
                        "Pool solution found"
                    );
                    valid_solution = Some(UcpSolution {
                        ucp: p_mid,
                        killed,
                        partial_fills: None,
                        reward_t0: reward
                    });
                }
                (
                    SupplyDemandResult::PartialFillEq {
                        bid_fill_q,
                        ask_fill_q,
                        reward_t0,
                        mut killed
                    },
                    ..
                ) => {
                    debug!(
                        ucp = ?p_mid,
                        partial = true,
                        reward_t0,
                        ?stats.amm_t0,
                        ?stats.amm_t1,
                        ?stats.order_t0,
                        ?stats.order_t1,
                        "Pool solution found"
                    );
                    killed.extend(perma_killed);

                    return Some(UcpSolution {
                        ucp: p_mid,
                        killed,
                        partial_fills: Some((bid_fill_q, ask_fill_q)),
                        reward_t0
                    });
                }
            }
        }
        if dust.is_some() {
            debug!("Solved with dust solution");
        } else {
            debug!("Unable to solve");
        }

        dust.map(|d| d.1)
    }
}

#[derive(Debug)]
struct UcpSolution {
    /// Solved uniform clearing price in T1/T0 format
    ucp:           Ray,
    /// Orders that were killed in this solution
    killed:        HashSet<OrderId>,
    /// Partial fill quantities in format `Option<(bid_partial, ask_partial)>
    /// lambda bid, lambda ask `
    partial_fills: Option<(u128, u128)>,
    /// Extra T0 that should be added to rewards
    reward_t0:     u128
}

struct CheckUcpStats {
    amm_t0:   I256,
    amm_t1:   I256,
    order_t0: I256,
    order_t1: I256
}

#[derive(Debug)]
pub enum SupplyDemandResult {
    /// There is more supply of our target token type than we can match, as well
    /// as orders that can be killed to establish balance at this price
    MoreSupply(Option<Vec<OrderId>>),
    /// There is more demand of our target token type than we can match, as well
    /// as orders that can be killed to establish balance at this price
    MoreDemand(Option<Vec<OrderId>>),
    /// Supply and Demand of our target token type are perfectly balanced!
    NaturallyEqual(HashSet<OrderId>, u128),
    /// We were able to balance supply and demand by using partial fills and
    /// allocating a reward.  Note that the reward will always be in T0, so when
    /// balancing for T0 we should not see a reward allocated
    /// the quantities here refer to the amount of partial fill needed
    PartialFillEq {
        bid_fill_q: u128,
        ask_fill_q: u128,
        reward_t0:  u128,
        killed:     HashSet<OrderId>
    }
}
