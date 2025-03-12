use alloy_primitives::{I256, U256};
use angstrom_types::{
    contract_payloads::tob::generate_current_price_adjusted_for_donation,
    matching::{
        SqrtPriceX96, get_quantities_at_price,
        uniswap::{Direction, PoolPrice, PoolPriceVec, Quantity}
    },
    orders::{NetAmmOrder, OrderFillState, OrderId, OrderOutcome, PoolSolution},
    sol_bindings::{
        RawPoolOrder, Ray,
        grouped_orders::{GroupedVanillaOrder, OrderWithStorageData},
        rpc_orders::TopOfBlockOrder
    }
};
use base64::Engine;
use tracing::trace;

use crate::OrderBook;

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
    pub fn new(
        book: &'a OrderBook,
        tob: Option<OrderWithStorageData<TopOfBlockOrder>>,
        fee: u128,
        solve_for_t0: bool
    ) -> Self {
        // Dump the book
        let json = serde_json::to_string(&(book, &tob)).unwrap();
        let b64_output = base64::prelude::BASE64_STANDARD.encode(json.as_bytes());
        trace!(data = b64_output, "Raw book data");

        let amm_start_price = if let Some(tob) = tob {
            if let Some(a) = book.amm() {
                let end = generate_current_price_adjusted_for_donation(&tob, a)
                    .expect("order structure should be valid here and never fail");

                Some(end)
            } else {
                None
            }
        } else {
            book.amm().map(|f| {
                let cost = (f.current_price() - Quantity::Token1(3852639724582632135_u128))
                    .unwrap()
                    .d_t0;
                // But then we have to operate in the right direction to calculate how much T1
                // we ACTUALLY get out
                let pricevec = (f.current_price() + Quantity::Token0(cost)).unwrap();
                pricevec.end_bound
            })
        };

        Self { book, amm_start_price, fee, solve_for_t0 }
    }

    fn fetch_concentrated_liquidity(&self, price: Ray) -> (I256, I256) {
        let Some(start_price) = self.amm_start_price.clone() else { return Default::default() };
        let Ok(end_price) = start_price.snapshot().at_price(SqrtPriceX96::from(price)) else {
            return Default::default()
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
    ) -> (I256, I256, Option<bool>, Option<u128>, Option<u128>, Option<OrderId>) {
        let mut t0_delta = I256::ZERO;
        let mut t1_delta = I256::ZERO;

        let mut extra_t0 = None;
        let mut extra_t1 = None;
        let mut side = None;
        let mut id = None;

        self.book
            .asks()
            .iter()
            .filter(|ask| price >= ask.pre_fee_price(self.fee) && ask.is_partial())
            .for_each(|ask| {
                if price == ask.price() {
                    // min prices
                    let (q_in_min, q_out_min) =
                        Self::get_amount_in_out(ask, ask.min_amount(), self.fee, price);
                    t0_delta += I256::try_from(q_in_min).unwrap();
                    t1_delta -= I256::try_from(q_out_min).unwrap();

                    // max amount
                    let (q_in_max, q_out_max) =
                        Self::get_amount_in_out(ask, ask.amount(), self.fee, price);
                    // if we are asks then
                    extra_t0 = Some(q_in_max - q_in_min);
                    extra_t1 = Some(q_out_max - q_out_min);
                    side = Some(true);
                    id = Some(ask.order_id);

                    return;
                }

                // max amount
                let (q_in, q_out) = Self::get_amount_in_out(ask, ask.amount(), self.fee, price);
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
                if price == bid.price().inv_ray_round(true) && extra_t0.is_none() {
                    let (q_in_min, q_out_min) =
                        Self::get_amount_in_out(bid, bid.min_amount(), self.fee, price);

                    t0_delta -= I256::try_from(q_out_min).unwrap();
                    t1_delta += I256::try_from(q_in_min).unwrap();

                    let (q_in_max, q_out_max) =
                        Self::get_amount_in_out(bid, bid.amount(), self.fee, price);
                    extra_t1 = Some(q_in_max - q_in_min);
                    extra_t0 = Some(q_out_max - q_out_min);
                    side = Some(false);

                    id = Some(bid.order_id);
                    return;
                }

                let (q_in, q_out) = Self::get_amount_in_out(bid, bid.amount(), self.fee, price);
                // given its a bid, q_in is always t1
                t0_delta -= I256::try_from(q_out).unwrap();
                t1_delta += I256::try_from(q_in).unwrap();
            });

        (t0_delta, t1_delta, side, extra_t0, extra_t1, id)
    }

    fn check_ucp(&self, price: Ray) -> SupplyDemandResult {
        let (book_t0, book_t1) = self.fetch_concentrated_liquidity(price);
        let (normal_t0, normal_t1) = self.fetch_amount_out_amount_in_non_partials(price);
        let (partial_t0, partial_t1, extra_is_ask, extra_t0, extra_t1, id) =
            self.fetch_amount_in_amount_out_partials(price);

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
            ?extra_is_ask,
            ?extra_t0,
            ?extra_t1,
            ?t0_sum,
            ?t1_sum,
            "Testing price"
        );

        if t0_sum.is_zero() && t1_sum.is_zero() {
            return SupplyDemandResult::NaturallyEqual;
        }

        // Depending on how we're solving, we want to look at a specific excess
        // liquidity
        let excess_liquidity = if self.solve_for_t0 { &t0_sum } else { &t1_sum };

        let (Some(is_ask), Some(extra_t0), Some(extra_t1), Some(id)) =
            (extra_is_ask, extra_t0, extra_t1, id)
        else {
            return if *excess_liquidity < I256::ZERO {
                SupplyDemandResult::MoreDemand
            } else {
                SupplyDemandResult::MoreSupply
            };
        };

        // If we're solving for t0, `bid` orders drain liquidity.  If we're solving for
        // T1, `ask` orders drain liquidity
        let partial_drains_liquidity = if self.solve_for_t0 { !is_ask } else { is_ask };
        // Use the appropriate extra amount depending on if we're solving for T0 or T1
        let available_partial_q = if self.solve_for_t0 { &extra_t0 } else { &extra_t1 };

        // means we have extra demand we can add.
        if *excess_liquidity > I256::ZERO && partial_drains_liquidity {
            tracing::info!(
                ?excess_liquidity,
                available_partial_q,
                "partial drains liquidity and excess_liquidity > 0"
            );
            // If we have positive excess liquidity, we need to drain it so we check to see
            // if our partial order can drain the entire amount we need to take care of
            if I256::try_from(*available_partial_q).unwrap() >= *excess_liquidity {
                return SupplyDemandResult::PartialFillEq { extra_fill_t0: *excess_liquidity, id };
            }
        } else if *excess_liquidity < I256::ZERO && !partial_drains_liquidity {
            tracing::info!(
                ?excess_liquidity,
                available_partial_q,
                "partial provides liquidity and excess_liquidity < 0"
            );
            // If we have negative excess liquidity, we need to fill it so we check to see
            // if our partial order can fill the entire amount we need to take care of
            if I256::try_from(*available_partial_q).unwrap() >= *excess_liquidity {
                return SupplyDemandResult::PartialFillEq {
                    extra_fill_t0: excess_liquidity.saturating_neg(),
                    id
                };
            }
        }

        tracing::info!(?t0_sum, ?t1_sum, ?price, self.solve_for_t0);
        if *excess_liquidity < I256::ZERO {
            SupplyDemandResult::MoreDemand
        } else {
            SupplyDemandResult::MoreSupply
        }
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
        // we can only have partial fills when ucp is exactly on.
        let (order_id, amount) = fetch.get_partial_unfill().unzip();
        let abs_partial_am = amount
            .map(|a| a.unsigned_abs().to::<u128>())
            .unwrap_or_default();

        self.book
            .bids()
            .iter()
            .map(|bid| OrderOutcome {
                id:      bid.order_id,
                outcome: if order_id == Some(bid.order_id) {
                    // Partial bids are always exact in so our bid partial is in T1.  Let's check if
                    // we need to flip this
                    let partial_amount = if self.solve_for_t0 {
                        // If we're solving for T0, our extra is T0, so we inverse this to T1
                        // rounding up
                        fetch.ucp.inverse_quantity(abs_partial_am, true)
                    } else {
                        // If we're solving for T1, this is all in T1 so no need to flip
                        abs_partial_am
                    };
                    trace!(
                        self.solve_for_t0,
                        min_amount = bid.min_amount(),
                        partial_amount,
                        abs_partial_am,
                        "Finding bid order partial amount"
                    );

                    OrderFillState::PartialFill(bid.min_amount() + partial_amount)
                } else if fetch.ucp <= bid.price().inv_ray_round(true) {
                    OrderFillState::CompleteFill
                } else {
                    OrderFillState::Unfilled
                }
            })
            .chain(self.book.asks().iter().map(|ask| OrderOutcome {
                id:      ask.order_id,
                outcome: if order_id == Some(ask.order_id) {
                    // Partial asks are always exact in so our ask partial is in T0.  Let's check if
                    // we need to flip this
                    let partial_amount = if self.solve_for_t0 {
                        // If we're solving for T0, this is all in T0 so no need to flip
                        abs_partial_am
                    } else {
                        // If we're solving for T1, we need to flip our partial amount from T1 to T0
                        fetch.ucp.inverse_quantity(abs_partial_am, true)
                    };

                    trace!(
                        self.solve_for_t0,
                        min_amount = ask.min_amount(),
                        partial_amount,
                        abs_partial_am,
                        "Finding ask order partial amount"
                    );

                    OrderFillState::PartialFill(ask.min_amount() + partial_amount)
                } else if fetch.ucp >= ask.price() {
                    OrderFillState::CompleteFill
                } else {
                    OrderFillState::Unfilled
                }
            }))
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
        let Some(price_and_partial_solution) = self.solve_clearing_price() else {
            tracing::info!("no solve");
            return PoolSolution {
                id: self.book.id(),
                searcher,
                limit: self
                    .book
                    .bids()
                    .iter()
                    .map(|o| OrderOutcome {
                        id:      o.order_id,
                        outcome: OrderFillState::Unfilled
                    })
                    .chain(self.book.asks().iter().map(|o| OrderOutcome {
                        id:      o.order_id,
                        outcome: OrderFillState::Unfilled
                    }))
                    .collect(),
                ..Default::default()
            };
        };

        let limit = self.fetch_orders_at_ucp(&price_and_partial_solution);

        let amm = self.fetch_amm_movement_at_ucp(price_and_partial_solution.ucp);

        PoolSolution {
            id: self.book.id(),
            ucp: price_and_partial_solution.ucp,
            amm_quantity: amm,
            limit,
            searcher
        }
    }

    fn solve_clearing_price(&self) -> Option<UcpSolution> {
        let ep = Ray::from(U256::from(1));
        let mut p_max = Ray::from(self.book.highest_clearing_price().saturating_add(*ep));
        let mut p_min = Ray::from(self.book.lowest_clearing_price().saturating_sub(*ep));
        println!("min: {p_min:?} max: {p_max:?}");

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
                        extra_t0_fill: None,
                        partial_id:    None
                    });
                }
                (SupplyDemandResult::PartialFillEq { extra_fill_t0, id }, _) => {
                    println!("solved based on sup, demand with partial order");
                    return Some(UcpSolution {
                        ucp:           p_mid,
                        extra_t0_fill: Some(extra_fill_t0),
                        partial_id:    Some(id)
                    });
                }
            }
        }

        None
    }
}

#[derive(Debug)]
struct UcpSolution {
    ucp:           Ray,
    // true means supply
    extra_t0_fill: Option<I256>,
    partial_id:    Option<OrderId>
}

impl UcpSolution {
    pub fn get_partial_unfill(&self) -> Option<(OrderId, I256)> {
        Some((self.partial_id?, self.extra_t0_fill?))
    }
}

#[derive(Debug)]
pub enum SupplyDemandResult {
    MoreSupply,
    MoreDemand,
    NaturallyEqual,
    PartialFillEq { extra_fill_t0: I256, id: OrderId }
}
