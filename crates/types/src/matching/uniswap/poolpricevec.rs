use std::{
    collections::HashMap,
    ops::{Deref, Neg}
};

use alloy::primitives::{I256, U256, Uint};
use eyre::{Context, eyre};
use uniswap_v3_math::{
    sqrt_price_math::{_get_amount_0_delta, _get_amount_1_delta},
    swap_math::compute_swap_step
};

use super::{
    Direction, DonationResult, LiqRangeRef, PoolSnapshot, Quantity, Tick, poolprice::PoolPrice
};
use crate::matching::{Ray, SqrtPriceX96, math::low_to_high};

#[derive(Clone, Debug)]
pub struct SwapStep<'a> {
    start_price: SqrtPriceX96,
    end_price:   SqrtPriceX96,
    d_t0:        u128,
    d_t1:        u128,
    liq_range:   LiqRangeRef<'a>
}

impl<'a> Deref for SwapStep<'a> {
    type Target = LiqRangeRef<'a>;

    fn deref(&self) -> &Self::Target {
        &self.liq_range
    }
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
            return Err(eyre!("Ticks out of bounds, unable to construct step"));
        }

        let bounded_low = if low_tick >= liq_range.lower_tick {
            *low
        } else {
            SqrtPriceX96::at_tick(liq_range.lower_tick)?
        };

        let bounded_high = if high_tick < liq_range.upper_tick {
            *high
        } else {
            SqrtPriceX96::at_tick(liq_range.upper_tick)?
        };

        if start > end {
            // If the price is decreasing go from high price to low price
            Self::compute_info(bounded_high, bounded_low, *liq_range)
        } else {
            // If the price is increasing go from low price to high price
            Self::compute_info(bounded_low, bounded_high, *liq_range)
        }
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
            return Err(eyre!("Price outside liquidity range"));
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

    /// Find the average settling price for this step, if the step is "empty"
    /// (no t0 or t1 was moved) we'll return None
    pub fn avg_price(&self) -> Option<Ray> {
        if self.empty() {
            None
        } else {
            Some(Ray::calc_price(U256::from(self.d_t0), U256::from(self.d_t1)))
        }
    }

    pub fn liquidity(&self) -> u128 {
        self.liq_range.liquidity
    }

    pub fn input(&self) -> u128 {
        if self.end_price > self.start_price { self.d_t1 } else { self.d_t0 }
    }

    pub fn output(&self) -> u128 {
        if self.end_price > self.start_price { self.d_t0 } else { self.d_t1 }
    }

    /// An empty step has no motion in t0 or t1, but is sometimes present to
    /// make sure we record a price that started precisely on a tick boundary
    pub fn empty(&self) -> bool {
        self.d_t0 == 0 || self.d_t1 == 0
    }
}

#[derive(Clone, Debug)]
pub struct PoolPriceVec<'a> {
    pub start_bound: PoolPrice<'a>,
    pub end_bound:   PoolPrice<'a>,
    pub d_t0:        u128,
    pub d_t1:        u128,
    pub steps:       Option<Vec<SwapStep<'a>>>
}

impl<'a> PoolPriceVec<'a> {
    /// Create a new PoolPriceVec from a start and end price with minimal
    /// checks.  The liquidity used will be the liquidity available at the
    /// `start_bound`.  This should be used in quick-and-dirty situations
    /// when we know that we're building a short-range PoolPriceVec that exists
    /// within a single liquidity position
    pub fn new(start: PoolPrice<'a>, end: PoolPrice<'a>) -> Self {
        Self::from_price_range(start.clone(), end.clone())
            .ok()
            .unwrap_or(Self {
                start_bound: start,
                end_bound:   end,
                d_t0:        0,
                d_t1:        0,
                steps:       None
            })
    }

    pub fn input(&self) -> u128 {
        if self.zero_for_one() { self.d_t0 } else { self.d_t1 }
    }

    pub fn output(&self) -> u128 {
        if self.zero_for_one() { self.d_t1 } else { self.d_t0 }
    }

    /// Returns the amount of T0 exchanged over this swap with a sign attached,
    /// negative if performing this swap consumes T0 (T0 is the input quantity
    /// for the described swap) and positive if performing this swap provides T0
    /// (T0 is the output quantity for the described swap)
    pub fn t0_signed(&self) -> I256 {
        let val = I256::unchecked_from(self.d_t0);
        if self.zero_for_one() { val.neg() } else { val }
    }

    /// Returns the amount of T1 exchanged over this swap with a sign attached,
    /// negative if performing this swap consumes T1 (T1 is the input quantity
    /// for the described swap) and positive if performing this swap provides T1
    /// (T1 is the output quantity for the described swap)
    pub fn t1_signed(&self) -> I256 {
        let val = I256::unchecked_from(self.d_t1);
        if self.zero_for_one() { val } else { val.neg() }
    }

    /// Returns a boolean indicating whether this PoolPriceVec is
    /// `zero_for_one`.  This will be true if the AMM is buying T0 and the AMM
    /// price is decreasing, false if the AMM is selling T0 and the AMM price is
    /// increasing.
    pub fn zero_for_one(&self) -> bool {
        self.start_bound.price > self.end_bound.price
    }

    pub fn steps(&self) -> Option<&Vec<SwapStep>> {
        self.steps.as_ref()
    }

    /// Return a reference to the underlying AMM snapshot that this PoolPriceVec
    /// is operating on
    pub fn snapshot(&self) -> &'a PoolSnapshot {
        self.start_bound.liq_range.pool_snap
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

    pub fn to_price_bound(start: PoolPrice<'a>, end_x96: SqrtPriceX96) -> eyre::Result<Self> {
        let end = start.snapshot().at_price(end_x96)?;
        Self::from_price_range(start, end)
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
        let mut total_out = U256::ZERO;
        let mut current_price = start.price;
        let mut current_liq_range: Option<_> = Some(start.liquidity_range());
        let mut left_to_swap = quantity.magnitude();

        let mut steps: Vec<SwapStep> = Vec::new();

        let is_swap_input = direction.is_input(&quantity);

        while left_to_swap > 0 {
            // Update our current liquidiy range
            let liq_range =
                current_liq_range.ok_or_else(|| eyre!("Unable to find next liquidity range"))?;
            // Compute our swap towards the appropriate end of our current liquidity bound
            let target_tick = liq_range.end_tick(direction);
            let target_price = SqrtPriceX96::at_tick(target_tick)?;
            // If our target price is equal to our current price, we're precisely at the
            // "bottom" of a liquidity range and we can skip this computation as
            // it will be a null step - but we're going to add the null step anyways for
            // donation purposes
            if target_price == current_price {
                steps.push(SwapStep {
                    start_price: current_price,
                    end_price: target_price,
                    d_t0: 0,
                    d_t1: 0,
                    liq_range
                });
                current_liq_range = liq_range.next(direction);
                continue;
            }

            let amount_remaining = if is_swap_input {
                // Exact in is calculated with a positive quantity
                I256::unchecked_from(left_to_swap)
            } else {
                // Exact out is calculated with a negative quantity
                I256::unchecked_from(left_to_swap).neg()
            };

            // Now we can compute our step
            let (fin_price, amount_in, amount_out, amount_fee) = compute_swap_step(
                current_price.into(),
                target_price.into(),
                liq_range.liquidity(),
                amount_remaining,
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
            if is_swap_input {
                // If our left_to_swap is the input, we want to subtract the amount in that was
                // allocated and the fee
                left_to_swap = left_to_swap.saturating_sub(amount_in.saturating_to());
                left_to_swap = left_to_swap.saturating_sub(amount_fee.saturating_to());
            } else {
                // If our left_to_swap is the output, we want to subtract the amount out that
                // was allocated
                left_to_swap = left_to_swap.saturating_sub(amount_out.saturating_to());
            }

            // Add the amount in and fee to our cost
            total_in += amount_in;
            total_in += amount_fee;
            // Add the amount out to our output
            total_out += amount_out;

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

    /// Builds a DonationResult based on the goal of first donating to liquidity
    /// ranges along this PriceVec until all liquidity ranges have effectively
    /// been swapped at the most beneficial price in the PriceVec, then
    /// distributing any remaining donation equally across all ranges in the
    /// PriceVec
    pub fn t0_donation(&self, total_donation: u128) -> DonationResult {
        tracing::trace!(total_donation, "Performing donation to end price");
        // If we have no steps we can just short-circuit this whole thing and take the
        // whole donation as tribute.  This will likely never happen.
        let Some(steps) = self.steps.as_ref() else {
            // in the case we don't cross any ticks
            return DonationResult {
                current_tick:   total_donation,
                tick_donations: HashMap::new(),
                final_price:    self.end_bound.price,
                total_donated:  0
            };
        };

        let mut remaining_donation = total_donation;
        let price_dropping = self.start_bound.price > self.end_bound.price;
        // The price will drop if we are adding T0 to the pool to get T1 out.  In these
        // cases we should round up.  If the price is increasing, we're atting T1 to the
        // pool to get T0 out and we should round down
        let round_up = price_dropping;

        let mut current_blob: Option<(u128, u128)> = None;

        let steps_iter = steps.iter().filter(|s| !s.empty() && s.is_initialized);
        tracing::info!(?steps_iter);

        for step in steps_iter {
            // If our current blob is empty, we can just insert the current step's stats
            // into it
            let Some((c_t0, c_t1)) = &mut current_blob else {
                current_blob = Some((step.d_t0, step.d_t1));
                continue;
            };

            // Find the average price of our current step and get our existing blob to
            // that price
            let target_price = step.avg_price().unwrap();
            let target_t0 = target_price.inverse_quantity(*c_t1, round_up);
            // The step cost is the difference between the amount of t0 we actually moved
            // and the amount we should have moved to be at this step's average price
            let step_cost = c_t0.abs_diff(target_t0);

            // If the move costs as much or less than what we have to spend, we've completed
            // this step and can merge blobs
            let step_complete = remaining_donation >= step_cost;

            let increment = std::cmp::min(remaining_donation, step_cost);
            if price_dropping {
                // If the price T1/T0 is dropping, we're going to be giving our LPs MORE T0 in
                // exchange for the T1 they pay us
                *c_t0 += increment;
            } else {
                // If the price T1/T0 is increasing, we're going to be refunding T0 to the LPs,
                // meaning they have effectively given us LESS T0 for the T1 we paid them
                *c_t0 = c_t0.saturating_sub(increment)
            }
            remaining_donation -= increment;

            if step_complete {
                // If we had enough reward to complete this step, we continue and merge this
                // step into the blob
                *c_t0 += step.d_t0;
                *c_t1 += step.d_t1;
            } else {
                // If we didn't have enough reward to complete this step, we're done
                break;
            }
        }

        // At this point, all of our swap is within the blob.  If we have additional
        // donation, we want to distribute it ALL to the blob to get to the best price
        // possible.
        if let Some((c_t0, _)) = current_blob.as_mut() {
            tracing::info!(?c_t0, ?remaining_donation, ?price_dropping);
            if price_dropping {
                *c_t0 += remaining_donation
            } else {
                *c_t0 = c_t0.saturating_sub(remaining_donation);
                if *c_t0 == 0 {
                    *c_t0 += 1;
                }
            }
        }
        tracing::info!(?current_blob);
        // Now we can find our filled price - if the price is dropping we want to round
        // down otherwise we want to round up.  Note that this diverges from other
        // rounding being done.
        let filled_price =
            current_blob.map(|(t0, t1)| Ray::calc_price_generic(t0, t1, !price_dropping));

        tracing::trace!(?filled_price, swap_end_price = ?self.end_bound.price, "Found post-donation price");
        // We've now found our filled price, we can allocate our reward to each tick
        // based on how much it costs to bring them to that price.
        // We can start remaining_donation over
        remaining_donation = total_donation;
        let mut total_donated = 0_u128;
        let tick_donations: HashMap<(Tick, Tick), (bool, u128)> = steps
            .iter()
            .filter(|step| step.is_initialized)
            .map(|step| {
                let reward = if let Some(f) = filled_price {
                    // T1 is constant, so we need to know how much t0 we need
                    let target_t0 = f.inverse_quantity(step.d_t1, round_up);
                    if price_dropping {
                        // If the filled_price should be lower than our current price, then our
                        // target T0 is MORE than we have in this step
                        std::cmp::min(remaining_donation, target_t0.saturating_sub(step.d_t0))
                    } else {
                        // If the filled_price should be higher than our current price, then our
                        // target T0 is LESS than we have in this step
                        std::cmp::min(remaining_donation, step.d_t0.saturating_sub(target_t0))
                    }
                } else {
                    0
                };
                remaining_donation -= reward;
                total_donated += reward;
                // We associate a reward with a specific liquidity range and we will extract the
                // lower or upper tick depending on the direction of our rewards
                ((step.liq_range.lower_tick(), step.liq_range.upper_tick()), (true, reward))
            })
            .collect();

        DonationResult {
            current_tick: remaining_donation,
            tick_donations,
            final_price: filled_price
                .map(|p| p.into())
                .unwrap_or_else(|| self.end_bound.as_sqrtpricex96()),
            total_donated
        }
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
            10,
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
        let pool = PoolSnapshot::new(10, vec![segment_1, segment_2], cur_price).unwrap();
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

    #[test]
    fn swaps_in_both_directions() {
        let seg_1_liq = 1_000_000_000_000_000_u128;
        let seg_2_liq = 1_000_000_000_000_u128;
        let segment_1 = LiqRange { liquidity: seg_1_liq, lower_tick: 100000, upper_tick: 100050 };
        let segment_2 = LiqRange { liquidity: seg_2_liq, lower_tick: 100050, upper_tick: 100100 };
        let segment_3 = LiqRange { liquidity: seg_1_liq, lower_tick: 100100, upper_tick: 100150 };
        let segment_4 = LiqRange { liquidity: seg_2_liq, lower_tick: 100150, upper_tick: 100200 };
        let cur_price = SqrtPriceX96::at_tick(100150).unwrap();
        let end_price = SqrtPriceX96::at_tick(100050).unwrap();
        let pool =
            PoolSnapshot::new(10, vec![segment_1, segment_2, segment_3, segment_4], cur_price)
                .unwrap();
        let low_start = pool.at_price(end_price).unwrap();
        let high_start = pool.current_price();

        let buy_vec = PoolPriceVec::from_swap(
            low_start,
            Direction::BuyingT0,
            Quantity::Token0(1234567890_u128)
        )
        .expect("Failed to generate pricevec from swap");

        assert!(buy_vec.d_t0 > 0, "No t0 moved in buy vec");
        assert!(buy_vec.d_t1 > 0, "No t1 moved in buy vec");

        let sell_vec = PoolPriceVec::from_swap(
            high_start,
            Direction::SellingT0,
            Quantity::Token0(1234567890_u128)
        )
        .expect("Failed to generate pricevec from swap");

        assert!(sell_vec.d_t0 > 0, "No t0 moved in buy vec");
        assert!(sell_vec.d_t1 > 0, "No t1 moved in buy vec");
    }

    #[test]
    fn will_include_all_steps() {
        let seg_1_liq = 1_000_000_000_000_000_u128;
        let seg_2_liq = 1_000_000_000_000_u128;
        let segment_1 = LiqRange { liquidity: seg_1_liq, lower_tick: 100000, upper_tick: 100050 };
        let segment_2 = LiqRange { liquidity: seg_2_liq, lower_tick: 100050, upper_tick: 100100 };
        let segment_3 = LiqRange { liquidity: seg_1_liq, lower_tick: 100100, upper_tick: 100150 };
        let segment_4 = LiqRange { liquidity: seg_2_liq, lower_tick: 100150, upper_tick: 100200 };
        let cur_price = SqrtPriceX96::at_tick(100150).unwrap();
        let end_price = SqrtPriceX96::at_tick(100050).unwrap();
        let pool =
            PoolSnapshot::new(10, vec![segment_1, segment_2, segment_3, segment_4], cur_price)
                .unwrap();
        let start_bound = pool.current_price();
        let end_bound = pool.at_price(end_price).unwrap();
        let pricevec = PoolPriceVec::from_price_range(start_bound.clone(), end_bound).unwrap();
        let steps = pricevec.steps.expect("Steps not generated");
        assert_eq!(steps.len(), 3, "Incorrect number of steps generated");

        let from_swap_vec = PoolPriceVec::from_swap(
            start_bound,
            Direction::SellingT0,
            Quantity::Token0(pricevec.d_t0)
        )
        .expect("Failed to generate pricevec from swap");
        let from_swap_steps = from_swap_vec.steps.expect("Steps not generated");
        assert_eq!(from_swap_steps.len(), 3, "Incorrect number of steps generated");
        assert_eq!(
            from_swap_vec.end_bound.price, pricevec.end_bound.price,
            "Final prices not equal"
        );
    }

    #[test]
    fn asset_swap_to_sqrt_works_t0() {
        let seg_1_liq = 1_000_000_000_000_000_u128;
        let seg_2_liq = 1_000_000_000_000_u128;
        let segment_1 = LiqRange { liquidity: seg_1_liq, lower_tick: 100000, upper_tick: 100050 };
        let segment_2 = LiqRange { liquidity: seg_2_liq, lower_tick: 100050, upper_tick: 100100 };
        let segment_3 = LiqRange { liquidity: seg_1_liq, lower_tick: 100100, upper_tick: 100150 };
        let segment_4 = LiqRange { liquidity: seg_2_liq, lower_tick: 100150, upper_tick: 100200 };
        let segment_5 = LiqRange { liquidity: seg_2_liq, lower_tick: 100200, upper_tick: 100250 };
        let cur_price = SqrtPriceX96::at_tick(100150).unwrap();
        let end_price = SqrtPriceX96::at_tick(100050).unwrap();
        let pool = PoolSnapshot::new(
            10,
            vec![segment_1, segment_2, segment_3, segment_4, segment_5],
            cur_price
        )
        .unwrap();

        let start = pool.current_price();
        let end = pool.at_price(end_price).unwrap();

        // check bid
        let r = PoolPriceVec::from_price_range(start.clone(), end).unwrap();
        // grab the bid quantity
        let bid_q = r.d_t0;
        // test with qty
        let s =
            PoolPriceVec::from_swap(start.clone(), Direction::SellingT0, Quantity::Token0(bid_q))
                .unwrap();
        let end_price_q = s.end_bound.price();
        assert_eq!(*end_price_q, end_price);

        // check ask
        let end_price = SqrtPriceX96::at_tick(100200).unwrap();
        let end = pool.at_price(end_price).unwrap();
        let r = PoolPriceVec::from_price_range(start.clone(), end).unwrap();
        let ask_q = r.d_t0;
        let s =
            PoolPriceVec::from_swap(start.clone(), Direction::BuyingT0, Quantity::Token0(ask_q))
                .unwrap();
        let end_price_q = s.end_bound.price();
        assert_eq!(*end_price_q, end_price);
    }

    #[test]
    fn asset_swap_to_sqrt_works_t1() {
        let seg_1_liq = 1_000_000_000_000_000_u128;
        let seg_2_liq = 1_000_000_000_000_u128;
        let segment_1 = LiqRange { liquidity: seg_1_liq, lower_tick: 100000, upper_tick: 100050 };
        let segment_2 = LiqRange { liquidity: seg_2_liq, lower_tick: 100050, upper_tick: 100100 };
        let segment_3 = LiqRange { liquidity: seg_1_liq, lower_tick: 100100, upper_tick: 100150 };
        let segment_4 = LiqRange { liquidity: seg_2_liq, lower_tick: 100150, upper_tick: 100200 };
        let segment_5 = LiqRange { liquidity: seg_2_liq, lower_tick: 100200, upper_tick: 100250 };
        let cur_price = SqrtPriceX96::at_tick(100150).unwrap();
        let end_price = SqrtPriceX96::at_tick(100050).unwrap();
        let pool = PoolSnapshot::new(
            10,
            vec![segment_1, segment_2, segment_3, segment_4, segment_5],
            cur_price
        )
        .unwrap();

        let start = pool.current_price();
        let end = pool.at_price(end_price).unwrap();

        // check bid
        let r = PoolPriceVec::from_price_range(start.clone(), end.clone()).unwrap();
        // grab the bid quantity
        let bid_q = r.d_t1;
        // test with qty
        let s =
            PoolPriceVec::from_swap(start.clone(), Direction::SellingT0, Quantity::Token1(bid_q))
                .unwrap();
        let end_price_q = s.end_bound.price();
        assert_eq!(*end_price_q, end_price);

        // check ask
        let end_price = SqrtPriceX96::at_tick(100200).unwrap();
        let end = pool.at_price(end_price).unwrap();
        let r = PoolPriceVec::from_price_range(start.clone(), end.clone()).unwrap();
        let ask_q = r.d_t1;
        let s =
            PoolPriceVec::from_swap(start.clone(), Direction::BuyingT0, Quantity::Token1(ask_q))
                .unwrap();
        let end_price_q = s.end_bound.price();
        assert_eq!(*end_price_q, end_price);
    }
}
