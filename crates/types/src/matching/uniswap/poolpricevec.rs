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
    matching::{Ray, SqrtPriceX96},
    orders::OrderPrice
};

#[derive(Clone, Debug)]
pub struct SwapStep<'a> {
    start_price: SqrtPriceX96,
    end_price:   SqrtPriceX96,
    d_t0:        U256,
    d_t1:        U256,
    liq_range:   LiqRangeRef<'a>
}

impl<'a> SwapStep<'a> {
    pub fn from_prices(start: PoolPrice<'a>, end: PoolPrice<'a>) -> eyre::Result<Self> {
        if start.liq_range != end.liq_range {
            return Err(eyre!(
                "A SwapStep can only cover one liquidity range, provided prices are from \
                 different ranges"
            ));
        }
        let liquidity = start.liquidity();
        let (round_0, round_1) = match Direction::from_prices(start.price, end.price) {
            Direction::BuyingT0 => (false, true),
            Direction::SellingT0 => (true, false)
        };
        let sqrt_ratio_a_x_96 = start.price.into();
        let sqrt_ratio_b_x_96 = end.price.into();
        let d_t0 = _get_amount_0_delta(sqrt_ratio_a_x_96, sqrt_ratio_b_x_96, liquidity, round_0)
            .unwrap_or(Uint::from(0));
        let d_t1 = _get_amount_1_delta(sqrt_ratio_a_x_96, sqrt_ratio_b_x_96, liquidity, round_1)
            .unwrap_or(Uint::from(0));
        Ok(Self {
            start_price: start.price,
            end_price: end.price,
            d_t0,
            d_t1,
            liq_range: start.liq_range
        })
    }

    pub fn start_price(&self) -> SqrtPriceX96 {
        self.start_price
    }

    pub fn end_price(&self) -> SqrtPriceX96 {
        self.end_price
    }

    pub fn avg_price(&self) -> Ray {
        Ray::calc_price(self.d_t0, self.d_t1)
    }

    pub fn liquidity(&self) -> u128 {
        self.liq_range.liquidity
    }

    pub fn input(&self) -> U256 {
        if self.end_price > self.start_price {
            self.d_t1
        } else {
            self.d_t0
        }
    }

    pub fn output(&self) -> U256 {
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
    pub d_t0:        U256,
    pub d_t1:        U256,
    steps:           Option<Vec<SwapStep<'a>>>
}

impl<'a> PoolPriceVec<'a> {
    pub fn new(start_bound: PoolPrice<'a>, end_bound: PoolPrice<'a>) -> Self {
        let (d_t0, d_t1) =
            Self::delta_to_price(start_bound.price, end_bound.price, start_bound.liquidity());
        Self { start_bound, end_bound, d_t0, d_t1, steps: None }
    }

    pub fn input(&self) -> U256 {
        if self.end_bound.price > self.start_bound.price {
            self.d_t1
        } else {
            self.d_t0
        }
    }

    pub fn output(&self) -> U256 {
        if self.end_bound.price > self.start_bound.price {
            self.d_t0
        } else {
            self.d_t1
        }
    }

    pub fn steps(&self) -> Option<&Vec<SwapStep>> {
        self.steps.as_ref()
    }

    pub fn from_price_range(start: PoolPrice<'a>, end: PoolPrice<'a>) -> eyre::Result<Self> {
        // If the two prices aren't from the same pool, we should error
        if !std::ptr::eq(start.liq_range.pool_snap, end.liq_range.pool_snap) {
            return Err(eyre!("Cannot create a price range from prices not in the same pool"));
        }
        let direction = Direction::from_prices(start.price, end.price);
        let mut cur_price = start.price;
        let mut cur_liq_range = Some(start.liq_range);

        while cur_price != end.price {
            // Update our current liquidiy range
            let liq_range =
                cur_liq_range.ok_or_else(|| eyre!("Unable to find next liquidity range"))?;
            // Compute our swap towards the appropriate end of our current liquidity bound
            let target_tick = liq_range.end_bound(direction);
            let target_price = SqrtPriceX96::at_tick(target_tick)?;
            // If our target price is equal to our current price, we're precisely at the
            // "bottom" of a liquidity range and we can skip this computation as
            // it will be a null step
            if target_price == cur_price {
                cur_liq_range = liq_range.next(direction);
                continue;
            }
            cur_price = target_price;
        }
        Ok(Self {
            start_bound: start,
            end_bound:   end,
            d_t0:        U256::ZERO,
            d_t1:        U256::ZERO,
            steps:       None
        })
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
            let target_tick = liq_range.end_bound(direction);
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
            let (d_t0, d_t1) = direction.sort_tokens(amount_in, amount_out);

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

        let (d_t0, d_t1) = direction.sort_tokens(total_in, total_out);
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
            let q_step = cur_q + step.output();
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
                    let tick_reward = tick_dprice.mul_quantity(step.output());
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

    fn delta_to_price(
        start_price: SqrtPriceX96,
        end_price: SqrtPriceX96,
        liquidity: u128
    ) -> (U256, U256) {
        let sqrt_ratio_a_x_96 = start_price.into();
        let sqrt_ratio_b_x_96 = end_price.into();
        let d_t0 = _get_amount_0_delta(sqrt_ratio_a_x_96, sqrt_ratio_b_x_96, liquidity, false)
            .unwrap_or(Uint::from(0));
        let d_t1 = _get_amount_1_delta(sqrt_ratio_a_x_96, sqrt_ratio_b_x_96, liquidity, false)
            .unwrap_or(Uint::from(0));
        (d_t0, d_t1)
    }

    pub fn is_buy(&self) -> bool {
        self.start_bound.price < self.end_bound.price
    }

    /// Returns `(quantity, price)`
    pub fn quantity(&self, target_price: OrderPrice) -> (U256, U256) {
        let t: SqrtPriceX96 = Ray::from(*target_price).into();

        // If our target price is past our end bound, our quantity is the entire range
        if (self.is_buy() && t > self.end_bound.price) || t < self.end_bound.price {
            return (self.d_t0, self.d_t1);
        }

        let (quantity, price) =
            Self::delta_to_price(self.start_bound.price, t, self.start_bound.liquidity());
        (quantity, price)
    }

    // Maybe it's OK that I don't check the price again here because in the matching
    // algo I've only offered a quantity bounded by the price, so we should
    // always be OK?
    pub fn fill(&self, quantity: U256) -> Self {
        let liquidity = self.start_bound.liquidity();
        let end_sqrt_price = if self.is_buy() {
            get_next_sqrt_price_from_output(
                self.start_bound.price.into(),
                liquidity,
                quantity,
                true
            )
            .map(SqrtPriceX96::from)
            .unwrap()
        } else {
            get_next_sqrt_price_from_input(self.start_bound.price.into(), liquidity, quantity, true)
                .map(SqrtPriceX96::from)
                .unwrap()
        };
        let (d_t0, d_t1) = Self::delta_to_price(self.start_bound.price, end_sqrt_price, liquidity);
        let mut end_bound = self.start_bound.clone();
        end_bound.price = end_sqrt_price;
        Self { end_bound, start_bound: self.start_bound.clone(), d_t0, d_t1, steps: None }
    }
}
