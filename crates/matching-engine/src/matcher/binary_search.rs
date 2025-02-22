use std::collections::hash_map::HashMap;

use alloy_primitives::{Address, U256};
use angstrom_types::{
    matching::{
        uniswap::{Direction, PoolPrice, PoolPriceVec, Quantity},
        SqrtPriceX96
    },
    orders::{NetAmmOrder, OrderFillState, OrderId, OrderOutcome, PoolSolution},
    sol_bindings::{
        grouped_orders::{GroupedVanillaOrder, OrderWithStorageData},
        rpc_orders::TopOfBlockOrder,
        RawPoolOrder, Ray
    }
};
use rand_distr::num_traits::ToPrimitive;

use crate::OrderBook;

#[derive(Clone)]
pub struct BinarySearchMatcher<'a> {
    book:            &'a OrderBook,
    /// changes if there is a tob or not
    amm_start_price: Option<PoolPrice<'a>>
}

impl<'a> BinarySearchMatcher<'a> {
    pub fn new(book: &'a OrderBook, tob: Option<OrderWithStorageData<TopOfBlockOrder>>) -> Self {
        let amm_start_price = if let Some(tob) = tob {
            if let Some(a) = book.amm() {
                let start = a.current_price();
                let direction = Direction::from_is_bid(tob.is_bid);
                let amount_out = tob.order.quantity_out;
                let q = if tob.is_bid {
                    Quantity::Token0(amount_out)
                } else {
                    Quantity::Token1(amount_out)
                };

                let r = PoolPriceVec::from_swap(start, direction, q).unwrap();

                Some(r.end_bound)
            } else {
                None
            }
        } else {
            book.amm().map(|f| f.current_price())
        };

        Self { book, amm_start_price }
    }

    // t0

    fn fetch_concentrated_liquidity_t0(&self, price: Ray, is_ask: bool) -> Ray {
        return Ray::default();
        let Some(start_price) = self.amm_start_price.clone() else { return Ray::default() };
        let start_sqrt = start_price.as_sqrtpricex96();
        let end_sqrt = SqrtPriceX96::from(price);

        let zfo = start_sqrt >= end_sqrt;

        // zfo = ask, so if they don't match, then we return zero
        if zfo ^ is_ask {
            return Ray::default()
        }

        let direction = Direction::from_is_bid(!zfo);

        let Ok(res) = PoolPriceVec::swap_to_price(start_price.clone(), end_sqrt, direction) else {
            return Ray::default()
        };

        // swap to price always returns the delta in y. if it is a bid, we search exact
        // // out else we search exact in
        Ray::from(U256::from(res.d_t0))
    }

    fn fetch_exact_in_ask_orders_t0(&self, price: Ray) -> Ray {
        self.book
            .asks()
            .iter()
            .filter(|ask| price >= ask.price() && ask.exact_in() && !ask.is_partial())
            .map(|ask| ask.fetch_supply_or_demand_contribution_with_fee_t0(price, 0))
            .sum()
    }

    fn fetch_exact_out_ask_orders_t0(&self, price: Ray) -> Ray {
        self.book
            .asks()
            .iter()
            .filter(|ask| price >= ask.price() && !ask.exact_in() && !ask.is_partial())
            .map(|ask| ask.fetch_supply_or_demand_contribution_with_fee_t0(price, 0))
            .sum()
    }

    fn fetch_exact_in_bid_orders_t0(&self, price: Ray) -> Ray {
        self.book
            .bids()
            .iter()
            .filter(|bid| {
                price <= bid.price().inv_ray_round(true) && bid.exact_in() && !bid.is_partial()
            })
            .map(|bid| bid.fetch_supply_or_demand_contribution_with_fee_t0(price, 0))
            .sum()
    }

    fn fetch_exact_out_bid_orders_t0(&self, price: Ray) -> Ray {
        self.book
            .bids()
            .iter()
            .filter(|bid| {
                price <= bid.price().inv_ray_round(true) && !bid.exact_in() && !bid.is_partial()
            })
            .map(|bid| bid.fetch_supply_or_demand_contribution_with_fee_t0(price, 0))
            .sum()
    }

    fn fetch_exact_in_partial_ask_t0(&self, price: Ray) -> PartialLiqRange {
        let mut removal = None;
        let mut id = None;

        let iter = self
            .book
            .asks()
            .iter()
            .filter(|ask| ask.is_partial() && price >= ask.price());

        assert!(iter.clone().is_sorted_by(|a, b| a.price() <= b.price()));

        let sum = iter
            .map(|ask| {
                let (amount, ex) =
                    ask.fetch_supply_or_demand_contribution_with_fee_partial_t0(price, 0);
                id = ex.is_some().then(|| ask.order_id);
                removal = ex;
                amount
            })
            .sum();

        PartialLiqRange {
            filled_quantity:      sum,
            optional_removal_liq: removal,
            optional_removal_id:  id
        }
    }

    fn fetch_exact_in_partial_bid_t0(&self, price: Ray) -> PartialLiqRange {
        let mut removal = None;
        let mut id = None;

        let iter = self
            .book
            .bids()
            .iter()
            .filter(|bid| bid.is_partial() && price <= bid.price().inv_ray_round(true));

        // assert sorting
        assert!(iter
            .clone()
            .is_sorted_by(|a, b| a.price().inv_ray_round(true) >= b.price().inv_ray_round(true)));

        let filled = iter
            .map(|bid| {
                let (amount, ex) =
                    bid.fetch_supply_or_demand_contribution_with_fee_partial_t0(price, 0);
                id = ex.is_some().then(|| bid.order_id);
                removal = ex;
                amount
            })
            .sum();

        PartialLiqRange {
            filled_quantity:      filled,
            optional_removal_liq: removal,
            optional_removal_id:  id
        }
    }

    fn total_supply_at_price_t0(&self, price: Ray) -> (Ray, Option<Ray>, Option<OrderId>) {
        let partial = self.fetch_exact_in_partial_ask_t0(price);
        (
            self.fetch_concentrated_liquidity_t0(price, true)
                + self.fetch_exact_in_ask_orders_t0(price)
                + self.fetch_exact_out_ask_orders_t0(price)
                + partial.filled_quantity,
            partial.optional_removal_liq,
            partial.optional_removal_id
        )
    }

    fn total_demand_at_price_t0(&self, price: Ray) -> (Ray, Option<Ray>, Option<OrderId>) {
        let partial = self.fetch_exact_in_partial_bid_t0(price);
        (
            self.fetch_concentrated_liquidity_t0(price, false)
                + self.fetch_exact_in_bid_orders_t0(price)
                + self.fetch_exact_out_bid_orders_t0(price)
                + partial.filled_quantity,
            partial.optional_removal_liq,
            partial.optional_removal_id
        )
    }

    /// calculates given the supply, demand, optional supply and optional demand
    /// what way the algo's price should move if we want it too
    pub fn calculate_solver_move_t0(&self, p_mid: Ray) -> SupplyDemandResult {
        let (total_supply, sub_sup, sub_id) = self.total_supply_at_price_t0(p_mid);
        let (total_demand, sub_demand, dem_id) = self.total_demand_at_price_t0(p_mid);

        cmp_total_supply_vs_demand(
            total_supply,
            total_demand,
            sub_sup.unwrap_or_default(),
            sub_id,
            sub_demand.unwrap_or_default(),
            dem_id
        )
    }

    // all of t1

    fn fetch_concentrated_liquidity_t1(&self, price: Ray, is_ask: bool) -> Ray {
        return Ray::default();
        let Some(start_price) = self.amm_start_price.clone() else { return Ray::default() };
        let start_sqrt = start_price.as_sqrtpricex96();
        let end_sqrt = SqrtPriceX96::from(price);

        let zfo = start_sqrt >= end_sqrt;

        // zfo = ask, so if they don't match, then we return zero
        if zfo ^ is_ask {
            return Ray::default()
        }

        let direction = Direction::from_is_bid(!zfo);

        let Ok(res) = PoolPriceVec::swap_to_price(start_price.clone(), end_sqrt, direction) else {
            return Ray::default()
        };

        // swap to price always returns the delta in y. if it is a bid, we search exact
        // // out else we search exact in
        Ray::from(U256::from(res.d_t1))
    }

    fn fetch_exact_in_ask_orders_t1(&self, price: Ray) -> Ray {
        self.book
            .asks()
            .iter()
            .filter(|ask| price >= ask.price() && ask.exact_in() && !ask.is_partial())
            .map(|ask| ask.fetch_supply_or_demand_contribution_with_fee_t1(price, 0))
            .sum()
    }

    fn fetch_exact_out_ask_orders_t1(&self, price: Ray) -> Ray {
        self.book
            .asks()
            .iter()
            .filter(|ask| price >= ask.price() && !ask.exact_in() && !ask.is_partial())
            .map(|ask| ask.fetch_supply_or_demand_contribution_with_fee_t1(price, 0))
            .sum()
    }

    fn fetch_exact_in_bid_orders_t1(&self, price: Ray) -> Ray {
        self.book
            .bids()
            .iter()
            .filter(|bid| {
                price <= bid.price().inv_ray_round(true) && bid.exact_in() && !bid.is_partial()
            })
            .map(|bid| bid.fetch_supply_or_demand_contribution_with_fee_t1(price, 0))
            .sum()
    }

    fn fetch_exact_out_bid_orders_t1(&self, price: Ray) -> Ray {
        self.book
            .bids()
            .iter()
            .filter(|bid| {
                price <= bid.price().inv_ray_round(true) && !bid.exact_in() && !bid.is_partial()
            })
            .map(|bid| bid.fetch_supply_or_demand_contribution_with_fee_t1(price, 0))
            .sum()
    }

    fn fetch_exact_in_partial_ask_t1(&self, price: Ray) -> PartialLiqRange {
        let mut removal = None;
        let mut id = None;

        let iter = self
            .book
            .asks()
            .iter()
            .filter(|ask| ask.is_partial() && price >= ask.price());

        assert!(iter.clone().is_sorted_by(|a, b| a.price() <= b.price()));

        let sum = iter
            .map(|ask| {
                let (amount, ex) =
                    ask.fetch_supply_or_demand_contribution_with_fee_partial_t1(price, 0);
                id = ex.is_some().then(|| ask.order_id);
                removal = ex;
                amount
            })
            .sum();

        PartialLiqRange {
            filled_quantity:      sum,
            optional_removal_liq: removal,
            optional_removal_id:  id
        }
    }

    fn fetch_exact_in_partial_bid_t1(&self, price: Ray) -> PartialLiqRange {
        let mut removal = None;
        let mut id = None;

        let iter = self
            .book
            .bids()
            .iter()
            .filter(|bid| bid.is_partial() && price <= bid.price().inv_ray_round(true));

        // assert sorting
        assert!(iter
            .clone()
            .is_sorted_by(|a, b| a.price().inv_ray_round(true) >= b.price().inv_ray_round(true)));

        let filled = iter
            .map(|bid| {
                let (amount, ex) =
                    bid.fetch_supply_or_demand_contribution_with_fee_partial_t1(price, 0);
                id = ex.is_some().then(|| bid.order_id);
                removal = ex;
                amount
            })
            .sum();

        PartialLiqRange {
            filled_quantity:      filled,
            optional_removal_liq: removal,
            optional_removal_id:  id
        }
    }

    // for t1, bids are supply
    fn total_supply_at_price_t1(&self, price: Ray) -> (Ray, Option<Ray>, Option<OrderId>) {
        let partial = self.fetch_exact_in_partial_bid_t1(price);
        (
            self.fetch_concentrated_liquidity_t1(price, false)
                + self.fetch_exact_in_bid_orders_t1(price)
                + self.fetch_exact_out_bid_orders_t1(price)
                + partial.filled_quantity,
            partial.optional_removal_liq,
            partial.optional_removal_id
        )
    }

    // for t1 asks are demand
    fn total_demand_at_price_t1(&self, price: Ray) -> (Ray, Option<Ray>, Option<OrderId>) {
        let partial = self.fetch_exact_in_partial_ask_t1(price);
        (
            self.fetch_concentrated_liquidity_t1(price, true)
                + self.fetch_exact_in_ask_orders_t1(price)
                + self.fetch_exact_out_ask_orders_t1(price)
                + partial.filled_quantity,
            partial.optional_removal_liq,
            partial.optional_removal_id
        )
    }

    /// calculates given the supply, demand, optional supply and optional demand
    /// what way the algo's price should move if we want it too
    pub fn calculate_solver_move_t1(&self, p_mid: Ray) -> SupplyDemandResult {
        let (total_supply, sub_sup, sub_id) = self.total_supply_at_price_t1(p_mid);
        let (total_demand, sub_demand, dem_id) = self.total_demand_at_price_t1(p_mid);

        cmp_total_supply_vs_demand_t1(
            total_supply,
            total_demand,
            sub_sup.unwrap_or_default(),
            sub_id,
            sub_demand.unwrap_or_default(),
            dem_id
        )
    }

    fn get_amount_in_out(
        order: &OrderWithStorageData<GroupedVanillaOrder>,
        fill_amount: u128,
        ray_ucp: Ray
    ) -> (u128, u128) {
        match (order.is_bid(), order.exact_in()) {
            // fill_amount is the exact amount of T1 being input to get a T0 output
            (true, true) => (
                // am in
                fill_amount,
                // am out - round down because we'll always try to give you less
                ray_ucp.inverse_quantity(fill_amount, false) - order.priority_data.gas.to::<u128>()
            ),
            // fill amount is the exact amount of T0 being output for a T1 input
            (true, false) => {
                // Round up because we'll always ask you to pay more
                (
                    ray_ucp.quantity(fill_amount + order.priority_data.gas.to::<u128>(), true),
                    fill_amount
                )
            }
            // fill amount is the exact amount of T0 being input for a T1 output
            (false, true) => {
                // Round down because we'll always try to give you less
                (
                    fill_amount,
                    ray_ucp.quantity(fill_amount - order.priority_data.gas.to::<u128>(), false)
                )
            }
            // fill amount is the exact amount of T1 expected out for a given T0 input
            (false, false) => (
                // Round up because we'll always ask you to pay more
                ray_ucp.inverse_quantity(fill_amount, true) + order.priority_data.gas.to::<u128>(),
                fill_amount
            )
        }
    }

    /// helper functions for grabbing all orders that we filled at ucp
    fn fetch_orders_at_ucp(&self, fetch: &UcpSolution) -> Vec<OrderOutcome> {
        // we can only have partial fills when ucp is exactly on.
        let (order_id, amount) = fetch.get_partial_unfill().unzip();
        let mut map: HashMap<Address, i128> = HashMap::new();
        let mut map2: HashMap<Address, i128> = HashMap::new();

        let res = self
            .book
            .bids()
            .iter()
            .map(|bid| OrderOutcome {
                id:      bid.order_id,
                outcome: if order_id == Some(bid.order_id) {
                    // partials are always exact in, so we need to convert this out
                    println!("{} - {} is bid: true", bid.amount(), amount.unwrap().to::<u128>());
                    // the amount here is always in Y. however for bids that are exact in, we want
                    // X
                    let partial_am = fetch.ucp.mul_quantity(*amount.unwrap()).to::<u128>();
                    let amount_in_partial = bid.amount() - partial_am;
                    let (amount_in, amount_out) =
                        Self::get_amount_in_out(bid, amount_in_partial, fetch.ucp);
                    *map.entry(bid.token_in()).or_default() += amount_in.to_i128().unwrap();
                    *map.entry(bid.token_out()).or_default() -= amount_out.to_i128().unwrap();

                    OrderFillState::PartialFill(amount_in_partial)
                } else if fetch.ucp <= bid.price().inv_ray_round(true) {
                    let (amount_in, amount_out) =
                        Self::get_amount_in_out(bid, bid.amount(), fetch.ucp);

                    *map.entry(bid.token_in()).or_default() += amount_in.to_i128().unwrap();
                    *map.entry(bid.token_out()).or_default() -= amount_out.to_i128().unwrap();
                    OrderFillState::CompleteFill
                } else {
                    OrderFillState::Unfilled
                }
            })
            .chain(self.book.asks().iter().map(|ask| OrderOutcome {
                id:      ask.order_id,
                outcome: if order_id == Some(ask.order_id) {
                    println!("{} - {} is bid: false", ask.amount(), amount.unwrap().to::<u128>());

                    let amount_parital = ask.amount() - amount.unwrap().to::<u128>();

                    let (amount_in, amount_out) =
                        Self::get_amount_in_out(ask, amount_parital, fetch.ucp);
                    *map2.entry(ask.token_in()).or_default() += amount_in.to_i128().unwrap();
                    *map2.entry(ask.token_out()).or_default() -= amount_out.to_i128().unwrap();
                    OrderFillState::PartialFill(amount_parital)
                } else if fetch.ucp >= ask.price() {
                    let (amount_in, amount_out) =
                        Self::get_amount_in_out(ask, ask.amount(), fetch.ucp);
                    *map2.entry(ask.token_in()).or_default() += amount_in.to_i128().unwrap();
                    *map2.entry(ask.token_out()).or_default() -= amount_out.to_i128().unwrap();
                    OrderFillState::CompleteFill
                } else {
                    OrderFillState::Unfilled
                }
            }))
            .collect::<Vec<_>>();

        for (k, v) in map2 {
            *map.entry(k).or_default() += v;
        }
        tracing::info!("order outcome\n\n\n {:#?}", map);

        res
    }

    fn fetch_amm_movement_at_ucp(&mut self, ucp: Ray) -> Option<NetAmmOrder> {
        return None;
        let Some(start_price) = self.amm_start_price.clone() else { return None };

        let start_sqrt = start_price.as_sqrtpricex96();
        let end_sqrt = SqrtPriceX96::from(ucp);
        let zfo = start_sqrt >= end_sqrt;
        let direction = Direction::from_is_bid(!zfo);

        let Ok(res) = PoolPriceVec::swap_to_price(start_price.clone(), end_sqrt, direction) else {
            return None
        };

        let mut tob_amm = NetAmmOrder::new(direction);
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
            }
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

    fn solve_clearing_price_using_t0_t1_intersection(&self) -> Option<UcpSolution> {
        let ep = Ray::from(U256::from(1));
        let mut p_max = Ray::from(self.book.highest_clearing_price().saturating_add(*ep));
        let mut p_min = Ray::from(self.book.lowest_clearing_price().saturating_sub(*ep));
        println!("min: {p_min:?} max: {p_max:?}");

        let two = U256::from(2);
        while (p_max - p_min) > ep {
            // grab all supply and demand
            let p_mid = (p_max + p_min) / two;

            // the delta of t0
            let res_t0 = self.calculate_solver_move_t0(p_mid);
            let res_t1 = self.calculate_solver_move_t1(p_mid);

            match (res_t0, res_t1) {
                // default cases.
                (SupplyDemandResult::MoreSupply, SupplyDemandResult::MoreSupply) => {
                    tracing::info!("both more supply");
                    p_max = p_mid;
                }
                (SupplyDemandResult::MoreDemand, SupplyDemandResult::MoreDemand) => {
                    tracing::info!("both more demand");
                    p_min = p_mid
                }
                (SupplyDemandResult::NaturallyEqual, SupplyDemandResult::NaturallyEqual) => {
                    println!("solved based on sup, demand no partials");
                    return Some(UcpSolution {
                        ucp:                   p_mid,
                        partial_unfill_amount: None,
                        partial_id:            None
                    })
                }
                (
                    SupplyDemandResult::PartialFillEq {
                        amount_unfilled: mut amount_unfilled_t0,
                        id: id_t0
                    },
                    SupplyDemandResult::PartialFillEq {
                        amount_unfilled: amount_unfilled_t1,
                        id: id_t1
                    }
                ) => {
                    // if different orders, then not really solvable
                    if id_t0 != id_t1 {
                        tracing::warn!(
                            "partial solve with different partial orders, shouldn't be possible"
                        );
                        return None
                    }

                    let expected_t1_unfill =
                        Ray::from(p_mid.quantity(amount_unfilled_t0.to::<u128>(), true));

                    let diff = amount_unfilled_t1.abs_diff(*expected_t1_unfill);

                    // we can always decrease the amount_unfilled_t0 as it approaches zero. this
                    // will make the delta smaller, only if expected_t1_unfill > amount_unfilled_t1
                    if expected_t1_unfill < amount_unfilled_t1 {
                        // given the diff in t1, we mul it back through to see if we can unfill
                        // less t0 to properly fill
                        let amount_diff =
                            Ray::from(p_mid.inverse_quantity(diff.to::<u128>(), false));

                        // if the total re-fill amount is greater than our avalable unfill, were
                        // good
                        if amount_diff <= amount_unfilled_t0 {
                            amount_unfilled_t0 -= amount_diff;
                        } else {
                            tracing::info!(
                                "the amount we need to fill t0 is more than the avalable unfill"
                            );
                            return None
                        }
                    } else {
                        tracing::info!("amount_unfilled_t1 > expected_t1_unfill, unsolvable");
                        return None
                    };

                    return Some(UcpSolution {
                        ucp:                   p_mid,
                        partial_unfill_amount: Some(amount_unfilled_t0),
                        partial_id:            Some(id_t0)
                    })

                    // return diff.is_zero().then(|| )
                }
                (SupplyDemandResult::MoreSupply, SupplyDemandResult::MoreDemand) => {
                    // cannot solve
                    tracing::warn!("t0 needs more supply and t1 needs more demand. unsolvable");
                    return None
                }
                (SupplyDemandResult::MoreDemand, SupplyDemandResult::MoreSupply) => {
                    tracing::warn!("t0 needs more demand and t1 needs more supply. unsolvable");
                    // cannot solve
                    return None
                }
                _ => {
                    tracing::warn!(
                        "any partial fill with a demand on or supply on the otherside isn't \
                         solvable"
                    );
                    return None
                }
            }
        }

        None
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
            let res = self.calculate_solver_move_t0(p_mid);

            match res {
                SupplyDemandResult::MoreSupply => {
                    p_max = p_mid;
                }
                SupplyDemandResult::MoreDemand => p_min = p_mid,
                SupplyDemandResult::NaturallyEqual => {
                    println!("solved based on sup, demand no partials");

                    return Some(UcpSolution {
                        ucp:                   p_mid,
                        partial_unfill_amount: None,
                        partial_id:            None
                    })
                }
                SupplyDemandResult::PartialFillEq { amount_unfilled, id } => {
                    println!("solved based on sup, demand with partial order");
                    return Some(UcpSolution {
                        ucp:                   p_mid,
                        partial_unfill_amount: Some(amount_unfilled),
                        partial_id:            Some(id)
                    })
                }
            }
        }

        None
    }
}

fn cmp_total_supply_vs_demand(
    total_supply: Ray,
    total_demand: Ray,
    sub_sup: Ray,
    sub_id: Option<OrderId>,
    sub_dem: Ray,
    dem_id: Option<OrderId>
) -> SupplyDemandResult {
    // if we can subtract the extra supply or demand and flip the equality, we have
    // reached ucp

    // seems to be were the problem is
    if (total_supply > total_demand && (total_supply - sub_sup) <= total_demand)
        || (total_supply < total_demand && (total_demand - sub_dem) <= total_supply)
    {
        let id = if total_supply > total_demand { sub_id } else { dem_id };

        let amount_unfilled = if total_supply > total_demand {
            println!(
                "total supply > total demand, sub {total_supply:?}
    {total_demand:?} {sub_sup:?}"
            );
            total_supply - total_demand
        } else {
            println!("total supply < total demand, sub");
            total_demand - total_supply
        };

        return SupplyDemandResult::PartialFillEq { amount_unfilled, id: id.unwrap() }
    }
    if total_supply > total_demand {
        SupplyDemandResult::MoreSupply
    } else if total_demand > total_supply {
        SupplyDemandResult::MoreDemand
    } else {
        SupplyDemandResult::NaturallyEqual
    }
}

fn cmp_total_supply_vs_demand_t1(
    total_supply: Ray,
    total_demand: Ray,
    sub_sup: Ray,
    sub_id: Option<OrderId>,
    sub_dem: Ray,
    dem_id: Option<OrderId>
) -> SupplyDemandResult {
    // if we can subtract the extra supply or demand and flip the equality, we have
    // reached ucp

    // seems to be were the problem is
    if (total_supply > total_demand && (total_supply - sub_sup) <= total_demand)
        || (total_supply < total_demand && (total_demand - sub_dem) <= total_supply)
    {
        let id = if total_supply > total_demand { sub_id } else { dem_id };

        let amount_unfilled = if total_supply > total_demand {
            println!(
                "total supply > total demand, sub {total_supply:?}
    {total_demand:?} {sub_sup:?}"
            );
            total_supply - total_demand
        } else {
            println!("total supply < total demand, sub");
            total_demand - total_supply
        };

        return SupplyDemandResult::PartialFillEq { amount_unfilled, id: id.unwrap() }
    }
    if total_supply < total_demand {
        SupplyDemandResult::MoreSupply
    } else if total_demand < total_supply {
        SupplyDemandResult::MoreDemand
    } else {
        SupplyDemandResult::NaturallyEqual
    }
}

#[derive(Debug)]
struct UcpSolution {
    ucp:                   Ray,
    // true means supply
    partial_unfill_amount: Option<Ray>,
    partial_id:            Option<OrderId>
}

impl UcpSolution {
    pub fn get_partial_unfill(&self) -> Option<(OrderId, Ray)> {
        Some((self.partial_id?, self.partial_unfill_amount?))
    }
}

#[derive(Debug)]
pub enum SupplyDemandResult {
    MoreSupply,
    MoreDemand,
    NaturallyEqual,
    PartialFillEq { amount_unfilled: Ray, id: OrderId }
}

#[derive(Debug, Default)]
struct PartialLiqRange {
    /// the quantity that filled up to the price.
    pub filled_quantity:      Ray,
    /// the amount of supply or demand that we can remove at the current ucp
    pub optional_removal_liq: Option<Ray>,
    // the order id
    pub optional_removal_id:  Option<OrderId>
}

#[cfg(test)]
pub mod test {

    use alloy::primitives::Uint;
    use angstrom_types::{
        matching::{uniswap::PoolSnapshot, Ray, SqrtPriceX96},
        primitive::PoolId
    };
    use testing_tools::type_generator::{
        amm::generate_amm_with_liquidity, orders::UserOrderBuilder
    };

    use crate::{book::OrderBook, matcher::binary_search::BinarySearchMatcher};

    // Helper function to create AMM with specific liquidity
    fn create_amm_at_price(price: Ray, liquidity: u128) -> PoolSnapshot {
        let sqrt_price: SqrtPriceX96 = price.into();
        let tick = sqrt_price.to_tick().unwrap();
        generate_amm_with_liquidity(tick - 10, tick + 10, liquidity, sqrt_price)
    }

    #[test]
    fn ask_side_double_match_works_with_amm_binary_search() {
        let pool_id = PoolId::random();
        let bid_price = Ray::from(Uint::from(1_000_000_000_u128)).inv_ray_round(true);
        let low_price = Ray::from(Uint::from(1_000_u128));

        let bid_order = UserOrderBuilder::new()
            .partial()
            .amount(10)
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
        let book = OrderBook::new(pool_id, None, vec![bid_order.clone()], vec![ask_order], None);
        println!("{:#?}", book);
        let matcher = BinarySearchMatcher::new(&book, None);
        let ucp = matcher.solve_clearing_price().unwrap().ucp;
        let bid = bid_price.inv_ray_round(true);
        assert!(
            ucp == bid,
            "Bid outweighed but the final price wasn't properly set {ucp:?}, {bid:?}"
        );
    }

    #[test]
    fn solve_bid_ex_out_ask_ex_in() {
        let pool_id = PoolId::random();
        let high_price = Ray::from(Uint::from(1_000_000_000_u128));
        let bid_order = UserOrderBuilder::new()
            .exact()
            .amount(100)
            .exact_in(false)
            .min_price(high_price)
            .with_storage()
            .bid()
            .build();

        let ask_order = UserOrderBuilder::new()
            .exact()
            .amount(100)
            .exact_in(true)
            .min_price(high_price)
            .with_storage()
            .ask()
            .build();
        let book = OrderBook::new(pool_id, None, vec![bid_order.clone()], vec![ask_order], None);
        let matcher = BinarySearchMatcher::new(&book, None);
        let ucp = matcher.solve_clearing_price().unwrap().ucp;
        assert!(
            ucp == high_price,
            "Ask outweighed but the final price wasn't properly set {ucp:?} {high_price:?}"
        );
    }

    #[test]
    fn solve_bid_ex_in_ask_ex_out() {
        let pool_id = PoolId::random();

        // 0.000000000000000001
        let high_price = Ray::from(Uint::from(1_000_000_000_u128));

        let bid_order = UserOrderBuilder::new()
            .exact()
            .amount(100)
            .exact_in(true)
            .min_price(high_price)
            .with_storage()
            .bid()
            .build();

        let ask_order = UserOrderBuilder::new()
            .exact()
            .amount(100)
            .exact_in(false)
            .min_price(high_price)
            .with_storage()
            .ask()
            .build();

        let book = OrderBook::new(pool_id, None, vec![bid_order.clone()], vec![ask_order], None);
        let matcher = BinarySearchMatcher::new(&book, None);
        let ucp = matcher.solve_clearing_price().unwrap().ucp;
        assert!(
            ucp == high_price,
            "Ask outweighed but the final price wasn't properly set {ucp:?} {high_price:?}"
        );
    }

    #[test]
    fn basic_solve_of_exact_orders_exact_out() {
        let pool_id = PoolId::random();
        // 0.0000000000001 rate
        let high_price = Ray::from(Uint::from(100_000_000_000_000u128));
        // both exact out so bid is 100 y

        // demand
        let bid_order = UserOrderBuilder::new()
            .exact()
            .amount(100_000_000_000_000)
            .exact_in(false)
            .min_price(high_price)
            .with_storage()
            .bid()
            .build();

        // supply
        let ask_order = UserOrderBuilder::new()
            .exact()
            .amount(10)
            .exact_in(false)
            .min_price(high_price)
            .with_storage()
            .ask()
            .build();
        let book = OrderBook::new(pool_id, None, vec![bid_order.clone()], vec![ask_order], None);
        let matcher = BinarySearchMatcher::new(&book, None);
        let ucp = matcher.solve_clearing_price().unwrap().ucp;

        assert!(
            ucp == high_price,
            "Ask outweighed but the final price wasn't properly set {ucp:?} {high_price:?}"
        );
    }

    #[test]
    fn test_amm_only_clearing() {
        let pool_id = PoolId::random();
        let price = Ray::from(Uint::from(1_000_000_000_u128)); // 1.0
        let amm = create_amm_at_price(price, 1_000_000); // Create AMM with significant liquidity

        let book = OrderBook::new(pool_id, Some(amm), vec![], vec![], None);
        let matcher = BinarySearchMatcher::new(&book, None);
        let ucp = matcher.solve_clearing_price().unwrap();

        println!("{ucp:?}");
        // let diff
        // assert!(
        //     (ucp - price).abs() <= Ray::from(U256::from(1)),
        //     "AMM-only clearing price should match initial price. Got {ucp:?},
        // expected {price:?}" );
    }

    #[test]
    fn test_amm_with_orders_clearing() {
        let pool_id = PoolId::random();
        let initial_price = Ray::from(Uint::from(1_000_000_000_u128)); // 1.0
        let amm = create_amm_at_price(initial_price, 1_000_000);

        // Add a large bid slightly below AMM price
        let bid_price = Ray::from(Uint::from(999_000_000_u128)); // 0.999
        let bid_order = UserOrderBuilder::new()
            .exact()
            .amount(2_000_000)
            .exact_in(true)
            .min_price(bid_price)
            .with_storage()
            .bid()
            .build();

        // Add a large ask slightly above AMM price
        let ask_price = Ray::from(Uint::from(1_001_000_000_u128)); // 1.001
        let ask_order = UserOrderBuilder::new()
            .exact()
            .amount(2_000_000)
            .exact_in(true)
            .min_price(ask_price)
            .with_storage()
            .ask()
            .build();

        let book = OrderBook::new(pool_id, Some(amm), vec![bid_order], vec![ask_order], None);
        let matcher = BinarySearchMatcher::new(&book, None);
        let ucp = matcher.solve_clearing_price().unwrap();

        // UCP should be between bid and ask prices due to AMM liquidity
        assert!(
            ucp.ucp >= bid_price && ucp.ucp <= ask_price,
            "Clearing price should be between bid and ask. Got {ucp:?}"
        );
    }

    #[test]
    fn test_amm_price_impact() {
        let pool_id = PoolId::random();
        let initial_price = Ray::from(Uint::from(1_000_000_000_u128)); // 1.0
        let amm = create_amm_at_price(initial_price, 100_000); // Lower liquidity for more price impact

        // Large bid order that should move price up
        let bid_price = Ray::from(Uint::from(1_100_000_000_u128)); // 1.1
        let bid_order = UserOrderBuilder::new()
            .exact()
            .amount(1_000_000)
            .exact_in(true)
            .min_price(bid_price)
            .with_storage()
            .bid()
            .build();

        let book = OrderBook::new(pool_id, Some(amm), vec![bid_order], vec![], None);
        let matcher = BinarySearchMatcher::new(&book, None);
        let ucp = matcher.solve_clearing_price().unwrap();

        assert!(
            ucp.ucp > initial_price,
            "Large bid should move price up from AMM initial price. Got {ucp:?}, initial was \
             {initial_price:?}"
        );
    }

    #[test]
    fn basic_solve_of_exact_orders_exact_in() {
        let pool_id = PoolId::random();
        // 0.0000000000001 rate
        let high_price = Ray::from(Uint::from(100_000_000_000_000u128));
        let bid_order = UserOrderBuilder::new()
            .exact()
            .amount(10)
            .exact_in(true)
            .min_price(high_price)
            .with_storage()
            .bid()
            .build();

        let ask_order = UserOrderBuilder::new()
            .exact()
            .amount(100_000_000_000_000)
            .exact_in(true)
            .min_price(high_price)
            .with_storage()
            .ask()
            .build();
        let book = OrderBook::new(pool_id, None, vec![bid_order.clone()], vec![ask_order], None);
        let matcher = BinarySearchMatcher::new(&book, None);
        let ucp = matcher.solve_clearing_price().unwrap().ucp;
        assert!(
            ucp == high_price,
            "Ask outweighed but the final price wasn't properly set {ucp:?} {high_price:?}"
        );
    }
}
