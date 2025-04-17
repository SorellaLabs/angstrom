use std::ops::Neg;

use alloy::primitives::{I256, U256};
use uniswap_v3_math::tick_math::{MAX_SQRT_RATIO, MIN_SQRT_RATIO};

use super::{donation::DonationCalculation, liquidity_base::LiquidityAtPoint};
use crate::{
    matching::{SqrtPriceX96, uniswap::Direction},
    sol_bindings::Ray
};

const U256_1: U256 = U256::from_limbs([1, 0, 0, 0]);

#[derive(Debug, Clone)]
pub struct PoolSwap<'a> {
    pub(super) liquidity:     LiquidityAtPoint<'a>,
    /// swap to sqrt price limit
    pub(super) target_price:  Option<SqrtPriceX96>,
    /// if its negative, it is an exact out.
    pub(super) target_amount: I256,
    /// what way to swap
    pub(super) direction:     Direction,
    // the fee of the pool.
    pub(super) fee:           u32
}

impl<'a> PoolSwap<'a> {
    pub fn swap(mut self) -> eyre::Result<PoolSwapResult<'a>> {
        let range_start = self.liquidity.current_sqrt_price;
        let range_start_tick = self.liquidity.current_tick;

        let exact_input = self.target_amount.is_positive();
        let sqrt_price_limit_x96 = self.target_price.map(|p| p.into()).unwrap_or_else(|| {
            if self.direction.is_ask() { MIN_SQRT_RATIO + U256_1 } else { MAX_SQRT_RATIO - U256_1 }
        });

        let mut amount_remaining = self.target_amount;
        let mut sqrt_price_x96: U256 = self.liquidity.current_sqrt_price.into();
        let mut total_in = U256::ZERO;
        let mut total_out = U256::ZERO;

        let mut steps = Vec::new();

        while amount_remaining != I256::ZERO && sqrt_price_x96 != sqrt_price_limit_x96 {
            let sqrt_price_start_x_96 = sqrt_price_x96;
            let start_tick = self.liquidity.current_tick;

            let (next_tick, liquidity, init) = self
                .liquidity
                .get_to_next_initialized_tick_within_one_word(self.direction.is_ask())?;

            let sqrt_price_next_x96 =
                uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick(next_tick)?;

            let target_sqrt_ratio = if (self.direction.is_ask()
                && sqrt_price_next_x96 < sqrt_price_limit_x96)
                || (!self.direction.is_ask() && sqrt_price_next_x96 > sqrt_price_limit_x96)
            {
                sqrt_price_limit_x96
            } else {
                sqrt_price_next_x96
            };

            let (new_sqrt_price_x_96, amount_in, amount_out, fee_amount) =
                uniswap_v3_math::swap_math::compute_swap_step(
                    sqrt_price_x96,
                    target_sqrt_ratio,
                    liquidity,
                    amount_remaining,
                    self.fee
                )?;

            sqrt_price_x96 = new_sqrt_price_x_96;
            if exact_input {
                // swap amount is positive so we sub
                amount_remaining = amount_remaining.saturating_sub(I256::from_raw(amount_in));
                amount_remaining = amount_remaining.saturating_sub(I256::from_raw(fee_amount));
            } else {
                // we add as is neg
                amount_remaining = amount_remaining.saturating_add(I256::from_raw(amount_out));
            }
            // add total in
            total_in += amount_in + fee_amount;
            total_out += amount_out;

            let (d_t0, d_t1) = self.direction.sort_tokens(amount_in.to(), amount_out.to());
            self.liquidity.move_to_next_tick(
                sqrt_price_x96,
                self.direction.is_ask(),
                sqrt_price_x96 == sqrt_price_next_x96,
                sqrt_price_x96 != sqrt_price_start_x_96
            )?;

            steps.push(PoolSwapStep {
                start_price: sqrt_price_start_x_96.into(),
                start_tick,
                end_price: sqrt_price_x96.into(),
                end_tick: next_tick,
                init,
                d_t0,
                d_t1
            });
        }

        // the final sqrt price
        self.liquidity.set_sqrt_price(sqrt_price_x96);

        let (total_d_t0, total_d_t1) = steps.iter().fold((0u128, 0u128), |(mut t0, mut t1), x| {
            t0 += x.d_t0;
            t1 += x.d_t1;
            (t0, t1)
        });

        Ok(PoolSwapResult {
            fee: self.fee,
            start_price: range_start,
            start_tick: range_start_tick,
            end_price: self.liquidity.current_sqrt_price,
            end_tick: self.liquidity.current_tick,
            total_d_t0,
            total_d_t1,
            steps,
            end_liquidity: self.liquidity
        })
    }
}

#[derive(Debug, Clone)]
pub struct PoolSwapResult<'a> {
    pub fee:           u32,
    pub start_price:   SqrtPriceX96,
    pub start_tick:    i32,
    pub end_price:     SqrtPriceX96,
    pub end_tick:      i32,
    pub total_d_t0:    u128,
    pub total_d_t1:    u128,
    pub steps:         Vec<PoolSwapStep>,
    pub end_liquidity: LiquidityAtPoint<'a>
}

impl<'a> PoolSwapResult<'a> {
    /// initialize a swap from the end of this swap into a new swap.
    pub fn swap_to_price(
        &'a self,
        amount: I256,
        direction: Direction,
        price_limit: Option<SqrtPriceX96>
    ) -> eyre::Result<PoolSwapResult<'a>> {
        PoolSwap {
            liquidity: self.end_liquidity.clone(),
            target_price: price_limit,
            direction,
            target_amount: amount,
            fee: self.fee
        }
        .swap()
    }

    pub fn t0_donation(&self, total_donation: u128) -> DonationCalculation {
        // if end price is lower, than is zfo
        let direction = self.start_price >= self.end_price;

        let round_up = direction;
        let mut remaining_donation = total_donation;

        let mut current_blob: Option<(u128, u128)> = None;

        for step in &self.steps {
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
            if direction {
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
            if direction {
                *c_t0 += remaining_donation
            } else {
                *c_t0 = c_t0.saturating_sub(remaining_donation);
                if *c_t0 == 0 {
                    *c_t0 += 1;
                }
            }
        }
        // get the filled price for the ranges
        let filled_price = current_blob.map(|(t0, t1)| Ray::calc_price_generic(t0, t1, !direction));

        // the amount we donate to the current tick.
        let mut current_tick_donation = remaining_donation;

        // the first range is the current range we in, it doesn't matter if
        // its initialized or no

        let donations_to_ticks = self
            .steps
            .iter()
            .filter(|s| s.init)
            .map(|step| {
                let reward = if let Some(f) = filled_price {
                    // T1 is constant, so we need to know how much t0 we need
                    let target_t0 = f.inverse_quantity(step.d_t1, round_up);
                    if direction {
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
                current_tick_donation -= reward;

                // because these are based off of direction, end tick will always be the tick we
                // swap to. withci
                (step.end_tick, reward)
            })
            .collect::<std::collections::HashMap<_, _>>();

        // inject current_tick
        // donations_to_ticks.insert(start_tick_range, current_tick_donation);

        DonationCalculation { rest: donations_to_ticks, total_donated: total_donation }
    }

    /// Returns the amount of T0 exchanged over this swap with a sign attached,
    /// negative if performing this swap consumes T0 (T0 is the input quantity
    /// for the described swap) and positive if performing this swap provides T0
    /// (T0 is the output quantity for the described swap)
    pub fn t0_signed(&self) -> I256 {
        let val = I256::unchecked_from(self.total_d_t0);
        if self.zero_for_one() { val.neg() } else { val }
    }

    /// Returns the amount of T1 exchanged over this swap with a sign attached,
    /// negative if performing this swap consumes T1 (T1 is the input quantity
    /// for the described swap) and positive if performing this swap provides T1
    /// (T1 is the output quantity for the described swap)
    pub fn t1_signed(&self) -> I256 {
        let val = I256::unchecked_from(self.total_d_t1);
        if self.zero_for_one() { val } else { val.neg() }
    }

    /// Returns a boolean indicating whether this PoolPriceVec is
    /// `zero_for_one`.  This will be true if the AMM is buying T0 and the AMM
    /// price is decreasing, false if the AMM is selling T0 and the AMM price is
    /// increasing.
    pub fn zero_for_one(&self) -> bool {
        self.start_price > self.end_price
    }

    pub fn input(&self) -> u128 {
        if self.zero_for_one() { self.total_d_t0 } else { self.total_d_t1 }
    }

    pub fn output(&self) -> u128 {
        if self.zero_for_one() { self.total_d_t1 } else { self.total_d_t0 }
    }
}

/// the step of swapping across this pool
#[derive(Clone, Debug)]
pub struct PoolSwapStep {
    start_price: SqrtPriceX96,
    start_tick:  i32,
    end_price:   SqrtPriceX96,
    end_tick:    i32,
    init:        bool,
    d_t0:        u128,
    d_t1:        u128
}

impl PoolSwapStep {
    pub fn avg_price(&self) -> Option<Ray> {
        if self.empty() {
            None
        } else {
            Some(Ray::calc_price(U256::from(self.d_t0), U256::from(self.d_t1)))
        }
    }

    pub fn empty(&self) -> bool {
        self.d_t0 == 0 || self.d_t1 == 0
    }
}
