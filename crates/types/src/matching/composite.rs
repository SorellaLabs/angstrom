use tracing::debug;

use super::{
    debt::Debt,
    math::amm_debt_same_move_solve,
    uniswap::{Direction, PoolPrice, PoolPriceVec, Quantity},
    Ray
};

#[derive(Clone, Debug, Default)]
pub struct CompositeOrder<'a> {
    debt:        Option<Debt>,
    amm:         Option<PoolPrice<'a>>,
    bound_price: Option<Ray>
}

impl<'a> CompositeOrder<'a> {
    pub fn new(debt: Option<Debt>, amm: Option<PoolPrice<'a>>, bound_price: Option<Ray>) -> Self {
        if debt.is_none() && amm.is_none() {
            panic!("Can't make a composite order with neither a debt nor an AMM");
        }
        Self { debt, amm, bound_price }
    }

    pub fn debt(&self) -> Option<&Debt> {
        self.debt.as_ref()
    }

    pub fn has_amm(&self) -> bool {
        self.amm.is_some()
    }

    pub fn has_debt(&self) -> bool {
        self.debt.is_some()
    }

    pub fn amm(&self) -> Option<&PoolPrice<'a>> {
        self.amm.as_ref()
    }

    pub fn bound(&self) -> Option<Ray> {
        self.bound_price
    }

    pub fn calc_quantities(&self, target_price: Ray) -> (u128, u128) {
        debug!(target_price = ?target_price, "Calculating quantities to target price");
        let amm_q = self
            .amm
            .as_ref()
            .map(|a| a.vec_to(target_price.into()).unwrap().d_t0)
            .unwrap_or_default();
        let debt_q = self
            .debt
            .map(|d| d.dq_to_price(&target_price))
            .unwrap_or_default();
        debug!(amm_q, debt_q, "Calculated quantities");
        if let Some(a) = self.amm.as_ref() {
            debug!(amm_price = ?Ray::from(a.price()), liquidity = a.liquidity(), "AMM final stats");
        }
        if let Some(d) = self.debt.as_ref() {
            debug!(debt_price = ?d.price(), "Debt final stats");
        }
        (amm_q, debt_q)
    }

    fn find_closest_bound(&self, external_bound: Ray) -> Ray {
        if let Some(ib) = self.bound_price {
            let cur_price = self.start_price();
            let external_dp = external_bound.abs_diff(*cur_price);
            let internal_dp = ib.abs_diff(*cur_price);
            if internal_dp < external_dp {
                ib
            } else {
                external_bound
            }
        } else {
            external_bound
        }
    }

    fn debt_direction(&self, target_price: Ray) -> Option<Direction> {
        self.debt
            .map(|d| Direction::from_prices(d.price(), target_price))
    }

    /// Return the quantity of t0 available to fill from this order to the
    /// target price.  If the quantity is equal to zero, we are in a "negative
    /// quantity" situation where the debt is on the Ask side and we have to
    /// do a "same side" match.  I'm pretty sure that's the only time that will
    /// happen
    pub fn quantity(&self, external_bound: Ray) -> u128 {
        // Check whether our external bound or internal bound is closer to our current
        // price
        let target_price = self.find_closest_bound(external_bound);
        // The quantity available to the target price is the combination of
        // the amount it takes to get our amm to the target price plus the
        // amount it takes to get our debt to the target price
        let (amm_q, debt_q) = self.calc_quantities(target_price);
        if let Some(Direction::BuyingT0) = self.debt_direction(target_price) {
            // If the price is going up, we're buying T0 from the AMM but our debt will be
            // providing less and less T0 so we subtract the `debt_q` from
            // the `amm_q` to determine how much T0 this composite order can
            // actually offer in liquidity
            amm_q.saturating_sub(debt_q)
        } else {
            // If the price is going down, we're selling T0 to the AMM and our debt will be
            // purchasing more and more T0 so we can just add the quantities
            // together to find the total liquidity consumed by both operations
            amm_q + debt_q
        }
    }

    /// Specifically in the case that we are buying T0, there's a "negative
    /// quantity" - the amount of T0 that is required to be provided from an
    /// external source as the T0 provided by the debt decreases with a price
    /// motion.  If an AMM is moving along with the debt, we can see if it
    /// provides an amount of T0 that offsets the debt's negative quantity.
    pub fn negative_quantity(&self, external_bound: Ray) -> u128 {
        let target_price = self.find_closest_bound(external_bound);
        let (amm_q, debt_q) = self.calc_quantities(target_price);
        if let Some(Direction::BuyingT0) = self.debt_direction(target_price) {
            debt_q.saturating_sub(amm_q)
        } else {
            0
        }
    }

    pub fn negative_quantity_t1(&self, external_bound: Ray) -> u128 {
        // cur_q - amm contribution * external_bound.inverse_quantity
        if let Some(d) = self.debt {
            let target_price = self.find_closest_bound(external_bound);
            let (amm_q, _) = self.calc_quantities(target_price);
            if let Some(Direction::BuyingT0) = self.debt_direction(target_price) {
                let t0_f = d.current_t0().saturating_sub(amm_q);
                let t1_f = target_price.quantity(t0_f, false);
                return t1_f.saturating_sub(d.magnitude());
            }
        }
        0
    }

    /// Compute the final state for the AMM and for the Debt when we partially
    /// fill this order with T1
    pub fn partial_fill_t1(&self, _partial_q_t1: u128) -> Self {
        self.clone()
    }

    /// Given an incoming amount of T0, determine how much of that T0 should go
    /// to the debt vs the AMM to ensure an equal movement of both
    /// quantities.  Works fine if we have only a debt or only an AMM
    pub fn t0_quantities(
        &self,
        t0_input: u128,
        direction: Direction
    ) -> (Option<u128>, Option<u128>) {
        match (self.amm.as_ref(), self.debt.as_ref()) {
            (None, None) => (None, None),
            (Some(_), None) => (Some(t0_input), None),
            (None, Some(_)) => (None, Some(t0_input)),
            (Some(a), Some(d)) => {
                let amm_portion = amm_debt_same_move_solve(
                    a.liquidity(),
                    d.current_t0(),
                    d.magnitude(),
                    t0_input,
                    direction
                );
                // Maybe build in some safety here around partial quantities
                let debt_portion = t0_input.saturating_sub(amm_portion);
                (Some(amm_portion), Some(debt_portion))
            }
        }
    }

    /// Compute the final state for the AMM and for the Debt when we partially
    /// fill this order.  The requirements for this final state are as follows:
    ///
    /// 1. The quantity filled is used precisely
    /// 2. The debt and the AMM end up at as close a price to each other as
    ///    possible
    pub fn partial_fill(&self, partial_q: u128, direction: Direction) -> Self {
        let (amm_quantity, debt_quantity) = self.t0_quantities(partial_q, direction);
        let new_amm = if let Some(amm_q) = amm_quantity {
            self.amm.clone().map(|a| {
                let quantity = Quantity::Token0(amm_q);
                PoolPriceVec::from_swap(a.clone(), direction, quantity)
                    .map(|v| v.end_bound)
                    .ok()
                    .unwrap_or_else(|| a.clone())
            })
        } else {
            self.amm.clone()
        };
        let new_debt = if let Some(debt_q) = debt_quantity {
            self.debt.map(|d| d.partial_fill(debt_q))
        } else {
            self.debt
        };
        Self { amm: new_amm, debt: new_debt, bound_price: self.bound_price }
    }

    /// Initial price of this composite order in Ray format.  Will default to
    /// the AMM price as it's more accurate, then step to the currently stored
    /// price on the Debt
    pub fn start_price(&self) -> Ray {
        self.amm
            .as_ref()
            .map(|a| a.as_ray())
            .or_else(|| self.debt.map(|d| d.price()))
            .unwrap()
    }
}

#[cfg(test)]
mod tests {

    use alloy_primitives::U256;

    use super::*;
    use crate::matching::{
        uniswap::{LiqRange, PoolSnapshot},
        DebtType, SqrtPriceX96
    };

    // Have to define this locally because testing_tools requires this crate and
    // that's rough
    fn simple_amm_at_tick(mid: i32, width: i32, liquidity: u128) -> PoolSnapshot {
        let amm_price = SqrtPriceX96::at_tick(mid).unwrap();
        let lower_tick = mid - width;
        let upper_tick = mid + width;
        let ranges = vec![LiqRange::new(lower_tick, upper_tick, liquidity).unwrap()];
        PoolSnapshot::new(ranges, amm_price).unwrap()
    }

    #[test]
    fn can_be_constructed() {
        let market = simple_amm_at_tick(100000, 100, 1_000_000_000_000_000_u128);
        let amm = market.current_price();
        let debt = Debt::new(DebtType::exact_in(100), Ray::default());
        // With just debt
        CompositeOrder::new(Some(debt), None, None);
        // With just AMM
        CompositeOrder::new(None, Some(amm.clone()), None);
        // With both
        CompositeOrder::new(Some(debt), Some(amm), None);
    }

    #[test]
    #[should_panic(expected = "Can't make a composite order with neither a debt nor an AMM")]
    fn will_not_construct_empty() {
        CompositeOrder::new(None, None, None);
    }

    #[test]
    fn computes_quantities() {
        let market = simple_amm_at_tick(100000, 100, 1_000_000_000_000_000_u128);
        let amm = market.current_price();
        let co = CompositeOrder::new(None, Some(amm), None);
        let target_price = Ray::from(SqrtPriceX96::at_tick(99990).unwrap());
        assert!(co.quantity(target_price) != 0, "Quantity of order was zero")
    }

    #[test]
    fn restricts_quantities_to_bounds() {
        let market = simple_amm_at_tick(100000, 100, 1_000_000_000_000_000_u128);
        let amm = market.current_price();
        let bound_price = Some(Ray::from(SqrtPriceX96::at_tick(100005).unwrap()));
        let co = CompositeOrder::new(None, Some(amm.clone()), bound_price);
        let target_price = Ray::from(SqrtPriceX96::at_tick(100010).unwrap());
        // Make sure we only went to the bound and not to the end
        let full_sweep = amm
            .vec_to(SqrtPriceX96::at_tick(100010).unwrap())
            .unwrap()
            .d_t0;
        let partial_sweep = amm
            .vec_to(SqrtPriceX96::at_tick(100005).unwrap())
            .unwrap()
            .d_t0;
        assert!(full_sweep > partial_sweep, "Full range improperly calculated");
        assert!(co.quantity(target_price) == partial_sweep, "CompositeOrder did not respect bound")
    }

    #[test]
    fn negative_quantities_are_zero() {
        let cur_price = Ray::from(SqrtPriceX96::at_tick(100000).unwrap());
        let bound_price = None;
        let target_price = Ray::from(SqrtPriceX96::at_tick(110000).unwrap());
        let debt = Debt::new(DebtType::exact_out(1_000_000_000), cur_price);
        let co = CompositeOrder::new(Some(debt), None, bound_price);
        assert!(co.quantity(target_price) == 0, "Quantity was not zero")
    }

    #[test]
    fn test_negative_quantity() {
        let market = simple_amm_at_tick(100000, 100, 1_000_000_000_000_000_u128);
        let amm = market.current_price();
        let cur_price = Ray::from(SqrtPriceX96::at_tick(100000).unwrap());
        let debt = Debt::new(DebtType::exact_out(2_000_000_000), cur_price);
        let target_price = Ray::from(SqrtPriceX96::at_tick(100010).unwrap());

        // Test with both AMM and debt
        let co = CompositeOrder::new(Some(debt), Some(amm), None);
        let neg_q = co.negative_quantity(target_price);
        assert!(neg_q > 0, "Should have negative quantity when debt exceeds AMM");
    }

    #[test]
    fn test_negative_quantity_t1() {
        let market = simple_amm_at_tick(100000, 100, 1_000_000_000_000_000_u128);
        let amm = market.current_price();
        let cur_price = Ray::from(SqrtPriceX96::at_tick(100000).unwrap());
        let debt = Debt::new(DebtType::exact_out(2_000_000_000), cur_price);
        let target_price = Ray::from(SqrtPriceX96::at_tick(100010).unwrap());

        let co = CompositeOrder::new(Some(debt), Some(amm.clone()), None);
        let neg_t1 = co.negative_quantity_t1(target_price);
        assert!(neg_t1 > 0, "Should have negative t1 quantity");

        // Test with no debt
        let co_no_debt = CompositeOrder::new(None, Some(amm), None);
        assert_eq!(co_no_debt.negative_quantity_t1(target_price), 0);
    }

    #[test]
    fn test_partial_fill() {
        let market = simple_amm_at_tick(100000, 100, 1_000_000_000_000_000_u128);
        let amm = market.current_price();
        let cur_price = Ray::from(SqrtPriceX96::at_tick(100000).unwrap());
        let debt = Debt::new(DebtType::exact_out(1_000_000_000), cur_price);
        let co = CompositeOrder::new(Some(debt.clone()), Some(amm.clone()), None);

        // Test partial fill with both AMM and debt
        let filled = co.partial_fill(500_000_000, Direction::BuyingT0);
        assert!(filled.debt().is_some());
        assert!(filled.amm().is_some());

        // Test partial fill with only AMM
        let co_amm = CompositeOrder::new(None, Some(amm), None);
        let filled_amm = co_amm.partial_fill(500_000_000, Direction::BuyingT0);
        assert!(filled_amm.amm().is_some());

        // Test partial fill with only debt
        let co_debt = CompositeOrder::new(Some(debt), None, None);
        let filled_debt = co_debt.partial_fill(500_000_000, Direction::BuyingT0);
        assert!(filled_debt.debt().is_some());
    }

    #[test]
    fn test_t0_quantities() {
        let market = simple_amm_at_tick(100000, 100, 1_000_000_000_000_000_u128);
        let amm = market.current_price();
        let cur_price = Ray::from(SqrtPriceX96::at_tick(100000).unwrap());
        let debt = Debt::new(DebtType::exact_out(1_000_000_000), cur_price);
        let co = CompositeOrder::new(Some(debt), Some(amm), None);

        // Test with both AMM and debt
        let (amm_q, debt_q) = co.t0_quantities(1_000_000_000, Direction::BuyingT0);
        assert!(amm_q.is_some());
        assert!(debt_q.is_some());
        assert!(amm_q.unwrap() + debt_q.unwrap() == 1_000_000_000);

        // Test with neither
        let (none_amm, none_debt) =
            CompositeOrder::new(None, None, None).t0_quantities(1_000_000_000, Direction::BuyingT0);
        assert!(none_amm.is_none());
        assert!(none_debt.is_none());
    }

    #[test]
    fn test_start_price() {
        let market = simple_amm_at_tick(100000, 100, 1_000_000_000_000_000_u128);
        let amm = market.current_price();
        let cur_price = Ray::from(SqrtPriceX96::at_tick(100000).unwrap());
        let debt = Debt::new(DebtType::exact_out(1_000_000_000), cur_price);

        // Test with both AMM and debt - should use AMM price
        let co = CompositeOrder::new(Some(debt.clone()), Some(amm.clone()), None);
        assert_eq!(co.start_price(), Ray::from(amm.price()));

        // Test with only debt
        let co_debt = CompositeOrder::new(Some(debt), None, None);
        assert_eq!(co_debt.start_price(), cur_price);
    }

    #[test]
    fn test_partial_fill_t1() {
        let market = simple_amm_at_tick(100000, 100, 1_000_000_000_000_000_u128);
        let amm = market.current_price();
        let cur_price = Ray::from(SqrtPriceX96::at_tick(100000).unwrap());
        let debt = Debt::new(DebtType::exact_out(1_000_000_000), cur_price);
        let co = CompositeOrder::new(Some(debt), Some(amm), None);

        let filled = co.partial_fill_t1(500_000_000);
        assert_eq!(filled.debt(), co.debt());
        assert_eq!(filled.amm(), co.amm());
    }

    #[test]
    fn test_find_closest_bound() {
        let market = simple_amm_at_tick(100000, 100, 1_000_000_000_000_000_u128);
        let amm = market.current_price();

        // Test with internal bound closer
        let internal_bound = Ray::from(SqrtPriceX96::at_tick(100005).unwrap());
        let external_bound = Ray::from(SqrtPriceX96::at_tick(100010).unwrap());
        let co = CompositeOrder::new(None, Some(amm.clone()), Some(internal_bound));
        assert_eq!(co.find_closest_bound(external_bound), internal_bound);

        // Test with external bound closer
        let internal_bound = Ray::from(SqrtPriceX96::at_tick(100015).unwrap());
        let co = CompositeOrder::new(None, Some(amm), Some(internal_bound));
        assert_eq!(co.find_closest_bound(external_bound), external_bound);
    }
}
