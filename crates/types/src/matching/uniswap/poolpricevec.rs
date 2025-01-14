use std::{cmp::Ordering, collections::HashMap};

use alloy::primitives::{Uint, I256, U256};
use eyre::{eyre, Context, OptionExt};
use uniswap_v3_math::{
    sqrt_price_math::{
        _get_amount_0_delta, _get_amount_1_delta, get_next_sqrt_price_from_input,
        get_next_sqrt_price_from_output
    },
    swap_math::compute_swap_step
};

use super::{poolprice::PoolPrice, Direction, LiqRangeRef, Quantity, Tick};
use crate::{
    matching::{math::low_to_high, Ray, SqrtPriceX96},
    orders::OrderPrice
};

#[derive(Clone, Debug)]
pub struct SwapStep<'a> {
    start_price: SqrtPriceX96,
    end_price:   SqrtPriceX96,
    d_t0:        u128,
    d_t1:        u128,
    liq_range:   LiqRangeRef<'a>
}

impl<'a> SwapStep<'a> {
    /// Create a SwapStep that covers the portion of the liquidity range
    /// contained within the bounds of the two prices given.  If the
    /// liquidity range is completely contained within the interval `[start,
    /// end)` then the generated SwapStep will cover the entire liquidity
    /// range.
    pub fn for_range(
        start: &PoolPrice<'a>,
        end: &PoolPrice<'a>,
        liq_range: &LiqRangeRef<'a>
    ) -> eyre::Result<Self> {
        Self::for_price_range(&start.price, &end.price, liq_range)
    }

    /// Create a SwapStep that covers the portion of the liquidity range
    /// contained within the bounds of the two prices given.  If the
    /// liquidity range is completely contained within the interval `[start,
    /// end)` then the generated SwapStep will cover the entire liquidity
    /// range.
    ///
    /// This method uses raw prices instead of PoolPrice references
    pub fn for_price_range(
        start: &SqrtPriceX96,
        end: &SqrtPriceX96,
        liq_range: &LiqRangeRef<'a>
    ) -> eyre::Result<Self> {
        // Sort our incoming prices into the low and high price
        let (low, high) = low_to_high(start, end);
        let low_tick = low.to_tick()?;
        let high_tick = high.to_tick()?;

        // Make sure both of our price ticks are within bounds, otherwise return an
        // error
        if low_tick >= liq_range.upper_tick || high_tick < liq_range.lower_tick {
            return Err(eyre!("Ticks out of bounds, unable to construct step"))
        }

        let start_price = if low_tick >= liq_range.lower_tick {
            *low
        } else {
            SqrtPriceX96::at_tick(liq_range.lower_tick)?
        };

        let end_price = if high_tick < liq_range.upper_tick {
            *high
        } else {
            SqrtPriceX96::at_tick(liq_range.upper_tick)?
        };

        Self::compute_info(start_price, end_price, *liq_range)
    }

    /// Creates a SwapStep that goes from the price given to the edge of the
    /// liquidity range that the price is associated with in the given Direction
    pub fn to_bound(start: PoolPrice<'a>, direction: Direction) -> eyre::Result<Self> {
        let end = start.liq_range.clone().end_price(direction);
        Self::from_prices(start, end)
    }

    /// Creates a SwapStep that covers the entirety of the provided liquidity
    /// range
    pub fn whole_range(range: LiqRangeRef<'a>, direction: Direction) -> eyre::Result<Self> {
        let start = range.start_price(direction);
        let end = range.end_price(direction);
        Self::from_prices(start, end)
    }

    /// Creates a SwapStep that covers the range between two prices, provided
    /// those prices are both within the same liquidity range
    pub fn from_prices(start: PoolPrice<'a>, end: PoolPrice<'a>) -> eyre::Result<Self> {
        if start.liq_range != end.liq_range {
            return Err(eyre!(
                "A SwapStep can only cover one liquidity range, provided prices are from \
                 different ranges"
            ));
        }
        Self::compute_info(start.price, end.price, start.liq_range)
    }

    /// Internal method for computing swap step details
    fn compute_info(
        start_price: SqrtPriceX96,
        end_price: SqrtPriceX96,
        liq_range: LiqRangeRef<'a>
    ) -> eyre::Result<Self> {
        // Make sure our prices are in the appropriate range.
        let (low_price, high_price) = low_to_high(&start_price, &end_price);
        // Low price is valid if it's within our liquidity range
        let low_price_valid = liq_range.price_in_range(*low_price);
        // High price is valid if it's either within our liquidity range or at the very
        // top of the liquidity range
        let high_price_valid = liq_range.price_in_range(*high_price)
            || *high_price == SqrtPriceX96::at_tick(liq_range.upper_tick).unwrap();
        if !(low_price_valid && high_price_valid) {
            return Err(eyre!("Price outside liquidity range"))
        }

        let liquidity = liq_range.liquidity;
        let (round_0, round_1) = match Direction::from_prices(start_price, end_price) {
            Direction::BuyingT0 => (false, true),
            Direction::SellingT0 => (true, false)
        };
        let sqrt_ratio_a_x_96 = start_price.into();
        let sqrt_ratio_b_x_96 = end_price.into();
        let d_t0 = _get_amount_0_delta(sqrt_ratio_a_x_96, sqrt_ratio_b_x_96, liquidity, round_0)
            .unwrap_or(Uint::from(0))
            .to();
        let d_t1 = _get_amount_1_delta(sqrt_ratio_a_x_96, sqrt_ratio_b_x_96, liquidity, round_1)
            .unwrap_or(Uint::from(0))
            .to();
        Ok(Self { start_price, end_price, d_t0, d_t1, liq_range })
    }

    pub fn start_price(&self) -> SqrtPriceX96 {
        self.start_price
    }

    pub fn end_price(&self) -> SqrtPriceX96 {
        self.end_price
    }

    pub fn avg_price(&self) -> Ray {
        Ray::calc_price(U256::from(self.d_t0), U256::from(self.d_t1))
    }

    pub fn liquidity(&self) -> u128 {
        self.liq_range.liquidity
    }

    pub fn input(&self) -> u128 {
        if self.end_price > self.start_price {
            self.d_t1
        } else {
            self.d_t0
        }
    }

    pub fn output(&self) -> u128 {
        if self.end_price > self.start_price {
            self.d_t0
        } else {
            self.d_t1
        }
    }
}

#[derive(Debug)]
pub struct DonationResult {
    pub tick_donations: HashMap<Tick, U256>,
    pub final_price:    SqrtPriceX96,
    pub total_donated:  u128,
    pub tribute:        u128
}

#[derive(Clone, Debug)]
pub struct PoolPriceVec<'a> {
    pub start_bound: PoolPrice<'a>,
    pub end_bound:   PoolPrice<'a>,
    pub d_t0:        u128,
    pub d_t1:        u128,
    steps:           Option<Vec<SwapStep<'a>>>
}

impl<'a> PoolPriceVec<'a> {
    /// Create a new PoolPriceVec from a start and end price with minimal
    /// checks.  The liquidity used will be the liquidity available at the
    /// `start_bound`.  This should be used in quick-and-dirty situations
    /// when we know that we're building a short-range PoolPriceVec that exists
    /// within a single liquidity position
    pub fn new(start_bound: PoolPrice<'a>, end_bound: PoolPrice<'a>) -> Self {
        let (d_t0, d_t1) =
            Self::delta_to_price(start_bound.price, end_bound.price, start_bound.liquidity());
        Self { start_bound, end_bound, d_t0, d_t1, steps: None }
    }

    pub fn input(&self) -> u128 {
        if self.end_bound.price > self.start_bound.price {
            self.d_t1
        } else {
            self.d_t0
        }
    }

    pub fn output(&self) -> u128 {
        if self.end_bound.price > self.start_bound.price {
            self.d_t0
        } else {
            self.d_t1
        }
    }

    pub fn steps(&self) -> Option<&Vec<SwapStep>> {
        self.steps.as_ref()
    }

    /// Create a new PoolPriceVec from a start and end price with full safety
    /// checks and with the ability to span liquidity boundaries.
    pub fn from_price_range(start: PoolPrice<'a>, end: PoolPrice<'a>) -> eyre::Result<Self> {
        // If the two prices aren't from the same pool, we should error
        if !std::ptr::eq(start.liq_range.pool_snap, end.liq_range.pool_snap) {
            return Err(eyre!("Cannot create a price range from prices not in the same pool"));
        }

        let steps: Vec<SwapStep> = start
            .liq_range
            .pool_snap
            .ranges_for_ticks(start.tick, end.tick)?
            .iter()
            .map(|liq_range| SwapStep::for_range(&start, &end, liq_range))
            .collect::<eyre::Result<Vec<SwapStep>>>()?;

        Self::from_steps(start, end, steps)
    }

    pub fn to_price_bound(start: PoolPrice<'a>, end: SqrtPriceX96) -> eyre::Result<Self> {
        let end_tick = end.to_tick()?;
        let steps = start
            .liq_range
            .pool_snap
            .ranges_for_ticks(start.tick, end_tick)?
            .iter()
            .map(|liq_range| {
                let (start, end) = low_to_high(&start.price, &end);
                SwapStep::for_price_range(start, end, liq_range)
            })
            .collect::<eyre::Result<Vec<SwapStep>>>()?;
        let end = if let Some(l) = steps.last() {
            let tick = l.end_price.to_tick()?;
            PoolPrice { liq_range: l.liq_range, price: l.end_price, tick }
        } else {
            return Err(eyre!("Unable to find actual end price"))
        };
        Self::from_steps(start, end, steps)
    }

    pub fn to_upper(start: PoolPrice<'a>) -> eyre::Result<Self> {
        let end = if let Some(range) = start
            .liq_range
            .pool_snap
            .get_range_for_tick(start.liq_range.upper_tick)
        {
            range.start_price(Direction::BuyingT0)
        } else {
            start.liq_range.end_price(Direction::BuyingT0)
        };
        let steps = vec![SwapStep::to_bound(start.clone(), Direction::BuyingT0)?];
        Self::from_steps(start, end, steps)
    }

    fn from_steps(
        start: PoolPrice<'a>,
        end: PoolPrice<'a>,
        steps: Vec<SwapStep<'a>>
    ) -> eyre::Result<Self> {
        let (d_t0, d_t1) = steps.iter().fold((0_u128, 0_u128), |(t0, t1), step| {
            (t0.saturating_add(step.d_t0), t1.saturating_add(step.d_t1))
        });
        Ok(Self { start_bound: start, end_bound: end, d_t0, d_t1, steps: Some(steps) })
    }

    pub fn from_swap(
        start: PoolPrice<'a>,
        direction: Direction,
        quantity: Quantity
    ) -> eyre::Result<Self> {
        let fee_pips = 0;
        let mut total_in = U256::ZERO;
        let mut current_price = start.price;
        let mut current_liq_range: Option<_> = Some(start.liquidity_range());
        let q = quantity.magnitude();

        let mut steps: Vec<SwapStep> = Vec::new();
        let total_out = U256::from(q);

        let mut remaining = I256::try_from(q).wrap_err_with(|| {
            // Should be impossible
            format!("Quantity too large to convert u128 -> I256: {}", q)
        })?;

        // "Exact out" is calculated with a negative quantity
        if !direction.is_input(&quantity) {
            remaining *= I256::MINUS_ONE;
        }

        while remaining < I256::ZERO {
            // Update our current liquidiy range
            let liq_range =
                current_liq_range.ok_or_else(|| eyre!("Unable to find next liquidity range"))?;
            // Compute our swap towards the appropriate end of our current liquidity bound
            let target_tick = liq_range.end_tick(direction);
            let target_price = SqrtPriceX96::at_tick(target_tick)?;
            // If our target price is equal to our current price, we're precisely at the
            // "bottom" of a liquidity range and we can skip this computation as
            // it will be a null step
            if target_price == current_price {
                current_liq_range = liq_range.next(direction);
                continue;
            }
            // Otherwise we can compute our step
            let (fin_price, amount_in, amount_out, amount_fee) = compute_swap_step(
                current_price.into(),
                target_price.into(),
                liq_range.liquidity(),
                remaining,
                fee_pips
            )
            .wrap_err_with(|| {
                format!(
                    "Unable to compute swap step from tick {:?} to {}",
                    current_price.to_tick(),
                    target_tick
                )
            })?;

            // See how much output we have yet to go
            let signed_out = I256::try_from(amount_out)
                .wrap_err("Output of step too large to convert U256 -> I256")?;
            remaining = remaining
                .checked_add(signed_out)
                .ok_or_eyre("Unable to add signed_out to expected_out")?;

            // Add the amount in and our total fee to our cost
            total_in += amount_in;
            total_in += amount_fee;

            // Based on our direction, sort out what our token0 and token1 are
            let (d_t0, d_t1) = direction.sort_tokens(amount_in.to(), amount_out.to());

            // Push this step onto our list of swap steps
            steps.push(SwapStep {
                start_price: current_price,
                end_price: SqrtPriceX96::from(fin_price),
                d_t0,
                d_t1,
                liq_range
            });
            // (avg_price, end_price, amount_out, liq_range));

            // If we're going to be continuing, move on to the next liquidity range
            current_liq_range = liq_range.next(direction);
            current_price = SqrtPriceX96::from(fin_price);
        }

        let (d_t0, d_t1) = direction.sort_tokens(total_in.to(), total_out.to());
        let end_bound = start.liq_range.pool_snap.at_price(current_price)?;
        Ok(Self { start_bound: start, end_bound, d_t0, d_t1, steps: Some(steps) })
    }

    pub fn donation(&self, q: u128) -> DonationResult {
        let mut remaining_donation = U256::from(q);
        let mut cur_q = U256::ZERO;
        let mut filled_price = self
            .steps
            .as_ref()
            .and_then(|v| v.first().map(|s| s.avg_price()))
            .unwrap_or_default();
        let empty = vec![];
        let steps = self.steps.as_ref().unwrap_or(&empty);
        let mut step_iter = steps.iter().peekable();
        while let Some(step) = step_iter.next() {
            let q_step = cur_q + U256::from(step.output());
            // Our target price is either the average price of the next stake or the end
            // price of the current stake if there's no next stake to deal with
            let target_price = step_iter
                .peek()
                .map(|next_stake| next_stake.avg_price())
                .unwrap_or_else(|| Ray::from(step.end_price));
            // The difference between this tick's average price and our target price
            let d_price = target_price - step.avg_price();

            // The step cost is the total cost in needed to ensure that all sold quantities
            // were sold at our target price
            let step_cost = d_price.mul_quantity(q_step);

            if remaining_donation >= step_cost {
                // If we have enough bribe to pay the whole cost, allocate that and step forward
                // to the next price gap
                cur_q = q_step;
                filled_price = target_price;
                remaining_donation -= step_cost;
            } else {
                // If we don't have enough bribe to pay the whole cost, figure out where the
                // target price winds up based on what we do have and end this iteration
                if remaining_donation > U256::ZERO {
                    let partial_dprice = Ray::calc_price(q_step, remaining_donation);
                    filled_price += partial_dprice;
                }
                break
            }
        }

        // We've now found our filled price, we can allocate our reward to each tick
        // based on how much it costs to bring them up to that price.
        let mut total_donated = U256::ZERO;
        let tick_donations: HashMap<Tick, U256> = steps
            .iter()
            //.filter_map(|(p_avg, _p_end, q_out, liq)| {
            .filter_map(|step| {
                // We always donate to the lower tick of our liquidity range as that is the
                // appropriate initialized tick to target
                let tick_num = step.liq_range.lower_tick();
                if filled_price > step.avg_price() {
                    let tick_dprice = filled_price - step.avg_price();
                    let tick_reward = tick_dprice.mul_quantity(U256::from(step.output()));
                    if tick_reward > U256::ZERO {
                        total_donated += tick_reward;
                        Some((tick_num, tick_reward))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();
        let tribute = q.saturating_sub(total_donated.saturating_to());
        DonationResult {
            tick_donations,
            final_price: self.end_bound.as_sqrtpricex96(),
            total_donated: total_donated.saturating_to(),
            tribute
        }
    }

    // Seems to be unused
    pub fn to_price(&self, target: SqrtPriceX96) -> Option<Self> {
        let (start_in_bounds, end_in_bounds) = if self.is_buy() {
            (Ordering::Greater, Ordering::Less)
        } else {
            (Ordering::Less, Ordering::Greater)
        };
        if self.start_bound.price.cmp(&target) == start_in_bounds {
            if self.end_bound.price.cmp(&target) == end_in_bounds {
                // If the target price is between the start and end bounds, make a subvec
                let new_upper = self.start_bound.liq_range.pool_snap.at_price(target).ok()?;
                Some(Self::new(self.start_bound.clone(), new_upper))
            } else {
                // If the target price is beyond the end bound in the appropriate direction,
                // return a copy of this existing vector
                Some(self.clone())
            }
        } else {
            // If the target price is equal to or beyond the start price in an inappropriate
            // direction, there is no vector to be made
            None
        }
    }

    /// A very raw delta to a specific price presuming the liquidity is constant
    /// for the duration of the swap
    fn delta_to_price(
        start_price: SqrtPriceX96,
        end_price: SqrtPriceX96,
        liquidity: u128
    ) -> (u128, u128) {
        let sqrt_ratio_a_x_96 = start_price.into();
        let sqrt_ratio_b_x_96 = end_price.into();
        let d_t0 = _get_amount_0_delta(sqrt_ratio_a_x_96, sqrt_ratio_b_x_96, liquidity, false)
            .unwrap_or(Uint::from(0));
        let d_t1 = _get_amount_1_delta(sqrt_ratio_a_x_96, sqrt_ratio_b_x_96, liquidity, false)
            .unwrap_or(Uint::from(0));
        (d_t0.to(), d_t1.to())
    }

    pub fn is_buy(&self) -> bool {
        self.start_bound.price < self.end_bound.price
    }

    /// Returns `(d_t0, d_t1, price)`
    pub fn quantity(&self, target_price: OrderPrice) -> (u128, u128, OrderPrice) {
        let t: SqrtPriceX96 = Ray::from(*target_price).into();

        // If our target price is past our end bound, our quantity is the entire range
        if (self.is_buy() && t > self.end_bound.price) || t < self.end_bound.price {
            return (self.d_t0, self.d_t1, OrderPrice::from(self.end_bound.price));
        }

        let (d_t0, d_t1) =
            Self::delta_to_price(self.start_bound.price, t, self.start_bound.liquidity());
        (d_t0, d_t1, target_price)
    }

    // Maybe it's OK that I don't check the price again here because in the matching
    // algo I've only offered a quantity bounded by the price, so we should
    // always be OK?
    pub fn fill(&self, quantity: u128) -> Self {
        let liquidity = self.start_bound.liquidity();
        let end_sqrt_price = if self.is_buy() {
            get_next_sqrt_price_from_output(
                self.start_bound.price.into(),
                liquidity,
                U256::from(quantity),
                true
            )
            .map(SqrtPriceX96::from)
            .unwrap()
        } else {
            get_next_sqrt_price_from_input(
                self.start_bound.price.into(),
                liquidity,
                U256::from(quantity),
                true
            )
            .map(SqrtPriceX96::from)
            .unwrap()
        };
        let (d_t0, d_t1) = Self::delta_to_price(self.start_bound.price, end_sqrt_price, liquidity);
        let mut end_bound = self.start_bound.clone();
        end_bound.price = end_sqrt_price;
        Self { end_bound, start_bound: self.start_bound.clone(), d_t0, d_t1, steps: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matching::uniswap::{LiqRange, PoolSnapshot};

    #[test]
    fn can_construct_pricevec() {
        let liquidity = 1_000_000_000_000_000_u128;
        let pool = PoolSnapshot::new(
            vec![LiqRange { liquidity, lower_tick: 100000, upper_tick: 100100 }],
            SqrtPriceX96::at_tick(100050).unwrap()
        )
        .unwrap();
        PoolPriceVec::new(pool.current_price(), pool.current_price());
    }

    #[test]
    fn will_span_segments() {
        let seg_1_liq = 1_000_000_000_000_000_u128;
        let seg_2_liq = 1_000_000_000_000_u128;
        let segment_1 = LiqRange { liquidity: seg_1_liq, lower_tick: 100000, upper_tick: 100050 };
        let segment_2 = LiqRange { liquidity: seg_2_liq, lower_tick: 100050, upper_tick: 100100 };
        let cur_price = SqrtPriceX96::at_tick(100025).unwrap();
        let end_price = SqrtPriceX96::at_tick(100075).unwrap();
        let pool = PoolSnapshot::new(vec![segment_1, segment_2], cur_price).unwrap();
        let start_bound = pool.current_price();
        let end_bound = pool.at_price(end_price).unwrap();
        let pricevec = PoolPriceVec::from_price_range(start_bound, end_bound).unwrap();
        assert!(pricevec.steps.is_some(), "No steps in our price vector");
        let steps = pricevec.steps.as_ref().unwrap();
        assert_eq!(steps.len(), 2, "Wrong number of steps in our price vector");

        // Make sure the total swap is the same
        let mid_price = SqrtPriceX96::at_tick(100050).unwrap();
        let amount_remaining = I256::unchecked_from(pricevec.d_t1);
        let (next_price, first_step_in, first_step_out, _) =
            compute_swap_step(cur_price.into(), mid_price.into(), seg_1_liq, amount_remaining, 0)
                .unwrap();
        assert_eq!(
            first_step_in,
            U256::from(steps[0].d_t1),
            "Token1 payment incorrect for first step"
        );
        assert_eq!(
            first_step_out,
            U256::from(steps[0].d_t0),
            "Token0 received incorrect for first step"
        );
        assert_eq!(SqrtPriceX96::from(next_price), mid_price, "Price after first step mismatched");
        let (post_second_price, second_step_in, second_step_out, _) =
            compute_swap_step(mid_price.into(), end_price.into(), seg_2_liq, amount_remaining, 0)
                .unwrap();
        assert_eq!(
            second_step_in,
            U256::from(steps[1].d_t1),
            "Token1 payment incorrect for second step"
        );
        assert_eq!(
            second_step_out,
            U256::from(steps[1].d_t0),
            "Token0 received incorrect for second step"
        );
        assert_eq!(
            SqrtPriceX96::from(post_second_price),
            end_price,
            "Price after second step mismatched"
        );

        assert_eq!(
            first_step_in + second_step_in,
            U256::from(pricevec.d_t1),
            "Final vec doesn't match input sum"
        );
        assert_eq!(
            first_step_out + second_step_out,
            U256::from(pricevec.d_t0),
            "Final vec doesn't match input sum"
        );
    }
}
