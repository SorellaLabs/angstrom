use super::{
    debt::Debt,
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

    fn calc_quantities(&self, target_price: Ray) -> (u128, u128) {
        let amm_q = self
            .amm
            .as_ref()
            .map(|a| a.vec_to(target_price.into()).unwrap().d_t0)
            .unwrap_or_default();
        let debt_q = self
            .debt
            .map(|d| d.dq_to_price(&target_price))
            .unwrap_or_default();
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

    /// Compute the final state for the AMM and for the Debt when we partially
    /// fill this order.  The requirements for this final state are as follows:
    ///
    /// 1. The quantity filled is used precisely
    /// 2. The debt and the AMM end up at as close a price to each other as
    ///    possible
    pub fn partial_fill(&self, partial_q: u128, direction: Direction) -> Self {
        // If we only have one thing (AMM or Debt) do a partial fill of that thing
        if self.amm.is_none() || self.debt.is_none() {
            // We can implement both maps here because it's all good
            let new_amm = self.amm.clone().map(|a| {
                let quantity = Quantity::Token0(partial_q);
                PoolPriceVec::from_swap(a.clone(), direction, quantity)
                    .map(|v| v.end_bound)
                    .ok()
                    .unwrap_or_else(|| a.clone())
            });
            let new_debt = self.debt.map(|d| d.partial_fill(partial_q));
            Self { amm: new_amm, debt: new_debt, bound_price: self.bound_price }
        } else {
            // If we have both the AMM AND Debt, we should do a combined fill of the two
            self.clone()
        }
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
}
