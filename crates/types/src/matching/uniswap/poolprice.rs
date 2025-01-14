use std::{
    cmp::{max, min},
    ops::{Add, Sub}
};

use alloy::primitives::U256;
use alloy_primitives::I256;
use eyre::eyre;
use malachite::rounding_modes::RoundingMode;
use uniswap_v3_math::{
    swap_math::compute_swap_step,
    tick_math::{get_sqrt_ratio_at_tick, get_tick_at_sqrt_ratio}
};

use super::{liqrange::LiqRangeRef, poolpricevec::PoolPriceVec, Direction, Quantity, Tick};
use crate::matching::{
    debt::Debt,
    math::{price_intersect_solve, resolve_precision},
    Ray, SqrtPriceX96
};
/// Representation of a specific price point in a Uniswap Pool.  Can be operated
/// on to simulate the behavior of the price withing said pool.
///
/// A PoolPrice represents a price based on the market state preserved in a
/// parent PoolSnapshot.  The PoolPrice can be moved and operated on to
/// simulate the behavior of the price if the underlying assets are bought and
/// sold.  This price will always depend on a specific PoolSnapshot so if
/// underlying parameters such as Liquidity or the decimal representation of the
/// assets were to change, you would need to procure a new PoolSnapshot and
/// new PoolPrices dependent on that.
#[derive(Clone, Debug)]
pub struct PoolPrice<'a> {
    /// Current PoolRange that the price is in
    pub(crate) liq_range: LiqRangeRef<'a>,
    /// Tick number within the current PoolRange that we're working with
    pub(crate) tick:      Tick,
    /// The ratio between Token0 and Token1
    pub(crate) price:     SqrtPriceX96
}

impl<'a> Eq for PoolPrice<'a> {}

impl<'a> PartialEq for PoolPrice<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.price.eq(&other.price) && self.liq_range == other.liq_range
    }
}

impl<'a> PartialOrd for PoolPrice<'a> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<'a> Ord for PoolPrice<'a> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.price.cmp(&other.price)
    }
}

impl<'a> PoolPrice<'a> {
    pub fn tick(&self) -> Tick {
        self.tick
    }

    pub fn liquidity_range(&self) -> LiqRangeRef<'a> {
        self.liq_range
    }

    pub fn liquidity(&self) -> u128 {
        self.liq_range.liquidity
    }

    /// Presuming a transaction in T0, return a new PoolPrice.  We error
    /// if we're not able to move enough in the direction we want to.
    pub fn d_t0(&self, quantity: u128, direction: Direction) -> eyre::Result<Self> {
        // We can short-circuit for a transaction of zero
        if quantity == 0 {
            return Ok(self.clone())
        }
        // Otherwise let's calculate
        let mut sqrt_ratio_current_x_96 = self.price.into();
        println!("Current price: {:?}", sqrt_ratio_current_x_96);
        let mut active_liq_range: Option<LiqRangeRef<'a>> = None;
        let mut cur_quantity = U256::from(quantity);
        while cur_quantity > U256::ZERO {
            println!("Starting new loop");
            // There might be a more suave way to do this
            let cur_liq_range = if let Some(lqr) = active_liq_range.as_mut() {
                println!("Bumping forward liquidity range");
                // If we already tested a liquidity range let's move to the next one
                let new_lqr = lqr.next(direction).ok_or_else(|| {
                    eyre!("Unable to find liquidity ranges that span the whole transaction")
                })?;
                *lqr = new_lqr;
                new_lqr
            } else {
                // Otherwise we can use the one we started in because this is the first
                // iteration
                active_liq_range = Some(self.liq_range);
                self.liq_range
            };
            let amount_remaining = match direction {
                // T0 when buying T0 is exact out (represented as a negative number in
                // compute_swap_step)
                Direction::BuyingT0 => I256::unchecked_from(cur_quantity) * I256::MINUS_ONE,
                // T0 when selling T0 is exact in (represented as a positive number in
                // compute_swap_step)
                Direction::SellingT0 => I256::unchecked_from(cur_quantity)
            };
            let sqrt_ratio_target_x_96 = cur_liq_range.end_price(direction).price.into();
            println!("Cur_q: {}, amount_remaining: {:?}", cur_quantity, amount_remaining);
            let (new_price, amount_in, amount_out, _) = compute_swap_step(
                sqrt_ratio_current_x_96,
                sqrt_ratio_target_x_96,
                cur_liq_range.liquidity(),
                amount_remaining,
                0
            )?;
            println!(
                "new_price: {:?}\namount_in: {}\namount_out: {}",
                new_price, amount_in, amount_out
            );
            // Update our current quantity
            match direction {
                Direction::BuyingT0 => cur_quantity -= amount_out,
                Direction::SellingT0 => cur_quantity -= amount_in
            }
            // Update our current price
            sqrt_ratio_current_x_96 = new_price;
        }
        // Convert our final elements into a new price element
        let price = SqrtPriceX96::from(sqrt_ratio_current_x_96);
        let tick = price.to_tick()?;
        let liq_range = active_liq_range.ok_or_else(|| {
            eyre!("Somehow have no active liquidity range despite iterationg - should never happen")
        })?;
        println!("End price: {:?}", price);
        Ok(PoolPrice { liq_range, price, tick })
    }

    /// Create a PoolPriceVec from the current price to a specific target price
    /// value within the associated Pool
    pub fn vec_to(&self, end_price: SqrtPriceX96) -> eyre::Result<PoolPriceVec<'a>> {
        PoolPriceVec::to_price_bound(self.clone(), end_price)
    }

    /// Create a PoolPriceVec from the current price to the upper bound of the
    /// liquidity range that the current price is in
    pub fn to_liq_range_upper(&self) -> eyre::Result<PoolPriceVec<'a>> {
        PoolPriceVec::to_upper(self.clone())
    }

    /// Create a PoolPriceVec from the current price to the lower bound of the
    /// liquidity range that the current price is in
    pub fn to_liq_range_lower(&self) -> eyre::Result<PoolPriceVec<'a>> {
        self.vec_to(SqrtPriceX96::at_tick(self.liq_range.lower_tick)?)
    }

    /// Determine the quantity of t0 needed to bring this price to the price of
    /// the given Debt
    pub fn intersect_with_debt(&self, debt: Debt) -> eyre::Result<u128> {
        // If the debt is already valid at our current price we can just move it, we're
        // done
        if debt.valid_for_price(self.as_ray()) {
            return Ok(0)
        }
        // Find out how much it would take to intersect with our debt presuming we stay
        // within our current liquidity range
        let vec_to_upper = self.to_liq_range_upper()?;
        let next_range_start = vec_to_upper.end_bound;
        let t0_to_upper = vec_to_upper.d_t0;
        let solve =
            price_intersect_solve(self.liquidity(), self.price, debt.magnitude(), debt.price());
        println!("Solve: {}", solve);
        let step = resolve_precision(192, solve, RoundingMode::Floor);
        println!("Step: {}", step);
        if step < t0_to_upper {
            return Ok(step)
        }
        let new_debt = debt.partial_fill(step);
        // If our next range is in another liquidity pool
        let recurse = if next_range_start.liq_range.lower_tick != self.liq_range.lower_tick {
            next_range_start.intersect_with_debt(new_debt)?
        } else {
            0
        };

        Ok(step + recurse)
    }

    /// This will produce a Uniswap Price Range that spans from the current
    /// price to the CLOSER of the target price or the nearest liquidity
    /// pool boundary
    ///
    /// This might not be needed anymore now that our poolpricevec can handle
    /// multiple liquidity pools
    pub fn order_to_target(
        &self,
        target_price: Option<SqrtPriceX96>,
        buy: bool
    ) -> Option<PoolPriceVec<'a>> {
        // Bounds check our target price if provided
        if let Some(p) = target_price {
            if buy {
                // Buying from the market will raise the price, so if our target price is on the
                // wrong side of our current price, we can't do this.
                if p <= self.price {
                    return None;
                }
            } else {
                // Selling to the market will lower the price, so the same applies here
                if p >= self.price {
                    return None;
                }
            }
        }

        let mut new_range_idx = self.liq_range.range_idx;
        let mut pool = self.liq_range.range;
        let (mut tick_bound_price, next_tick) = if buy {
            let upper_tick_price = get_sqrt_ratio_at_tick(pool.upper_tick)
                .ok()
                .map(SqrtPriceX96::from)?;
            let next_tick = self.liq_range.range_idx.checked_sub(1);
            (upper_tick_price, next_tick)
        } else {
            let lower_tick_price = get_sqrt_ratio_at_tick(pool.lower_tick)
                .ok()
                .map(SqrtPriceX96::from)?;
            let next_tick = self.liq_range.range_idx.checked_add(1);
            (lower_tick_price, next_tick)
        };
        if self.price == tick_bound_price {
            // We're at the tick bound, we need to look at the next pool!
            new_range_idx = next_tick?;
            pool = self.liq_range.pool_snap.ranges.get(new_range_idx)?;
            tick_bound_price = if buy {
                get_sqrt_ratio_at_tick(pool.upper_tick)
                    .ok()
                    .map(SqrtPriceX96::from)?
            } else {
                get_sqrt_ratio_at_tick(pool.lower_tick)
                    .ok()
                    .map(SqrtPriceX96::from)?
            };
        }
        let closest_price = if let Some(p) = target_price {
            if buy {
                min(p, tick_bound_price)
            } else {
                max(p, tick_bound_price)
            }
        } else {
            tick_bound_price
        };
        let end_bound = Self {
            liq_range: LiqRangeRef { range: pool, range_idx: new_range_idx, ..self.liq_range },
            price:     closest_price,
            tick:      get_tick_at_sqrt_ratio(closest_price.into()).ok()?
        };
        Some(PoolPriceVec::new(self.clone(), end_bound))
    }

    pub fn price(&self) -> &SqrtPriceX96 {
        &self.price
    }

    /// Return the current price as a Ray
    pub fn as_ray(&self) -> Ray {
        Ray::from(self.price)
    }

    /// Return the current SqrtPriceX96 structure
    pub fn as_sqrtpricex96(&self) -> SqrtPriceX96 {
        self.price
    }

    /// Return the current price (NOT sqrt) as a float by calling SqrtPriceX86's
    /// `as_f64` method
    pub fn as_float(&self) -> f64 {
        self.price.as_f64()
    }
}

impl<'a> Add<Quantity> for PoolPrice<'a> {
    type Output = eyre::Result<PoolPriceVec<'a>>;

    fn add(self, rhs: Quantity) -> Self::Output {
        PoolPriceVec::from_swap(self, rhs.as_input(), rhs)
    }
}

impl<'a> Sub<Quantity> for PoolPrice<'a> {
    type Output = eyre::Result<PoolPriceVec<'a>>;

    fn sub(self, rhs: Quantity) -> Self::Output {
        PoolPriceVec::from_swap(self, rhs.as_output(), rhs)
    }
}

impl<'a> From<PoolPrice<'a>> for U256 {
    fn from(value: PoolPrice) -> Self {
        value.price.into()
    }
}

#[cfg(test)]
mod test {
    use alloy_primitives::U160;

    use crate::matching::{
        uniswap::{Direction, LiqRange, PoolSnapshot},
        Debt, Ray, SqrtPriceX96
    };

    #[test]
    fn intersects_with_debt() {
        let debt_price = Ray::from(SqrtPriceX96::at_tick(100000).unwrap());
        let debt = Debt::new(crate::matching::DebtType::ExactOut(1_000_000_000_u128), debt_price);
        let amm = PoolSnapshot::new(
            vec![LiqRange {
                liquidity:  1_000_000_000_u128,
                lower_tick: 99900,
                upper_tick: 100100
            }],
            SqrtPriceX96::at_tick(100001).unwrap()
        )
        .unwrap();
        let result = amm.current_price().intersect_with_debt(debt).unwrap();
        println!("Result: {}", result);
        let valid = debt.valid_for_price(amm.current_price().as_ray());
        println!("Valid: {}", valid);
    }

    #[test]
    fn can_buy_and_sell_t0() {
        let amm = PoolSnapshot::new(
            vec![
                LiqRange {
                    liquidity:  1_000_000_000_000_u128,
                    lower_tick: 99900,
                    upper_tick: 100100
                },
                LiqRange {
                    liquidity:  1_000_000_000_000_000_u128,
                    lower_tick: 100100,
                    upper_tick: 100200
                },
            ],
            SqrtPriceX96::at_tick(100099).unwrap()
        )
        .unwrap();
        let cur_price = amm.current_price();
        let new_price = amm
            .current_price()
            .d_t0(1000000, Direction::BuyingT0)
            .unwrap();
        assert!(new_price.price > cur_price.price, "Price didn't move up when buying T0");
        let third_price = new_price.d_t0(1000000, Direction::SellingT0).unwrap();
        let diff = third_price.price.abs_diff(*cur_price.price);
        assert!(diff <= U160::from(1_u128), "Price didn't move back when selling T0");
    }
}
