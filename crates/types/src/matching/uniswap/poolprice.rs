use std::{
    cmp::{max, min},
    ops::{Add, Sub}
};

use alloy::primitives::U256;
use eyre::OptionExt;
use uniswap_v3_math::tick_math::{get_sqrt_ratio_at_tick, get_tick_at_sqrt_ratio};

use super::{liqrange::LiqRangeRef, poolpricevec::PoolPriceVec, Quantity, Tick};
use crate::matching::SqrtPriceX96;
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

    pub fn range_to(&self, price: SqrtPriceX96) -> eyre::Result<PoolPriceVec<'a>> {
        let tick = price.to_tick()?;
        let market_pool = self
            .liq_range
            .pool_snap
            .get_range_for_tick(tick)
            .ok_or_eyre("Unable to find pool")?;
        let target = Self { liq_range: market_pool, tick, price };
        Ok(PoolPriceVec::new(self.clone(), target))
    }

    /// This will produce a Uniswap Price Range that spans from the current
    /// price to the CLOSER of the target price or the nearest liquidity
    /// pool boundary
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

    /// Return the current SqrtPriceX96 structure
    pub fn as_sqrtpricex96(&self) -> SqrtPriceX96 {
        self.price
    }

    /// Return the current price (NOT sqrt) as a float by calling SqrtPriceX86's
    /// `as_f64` method
    pub fn as_float(&self) -> f64 {
        self.price.as_f64()
    }

    /// Return the current SqrtPriceX96 as a U256 without conversion
    pub fn as_u256(&self) -> U256 {
        self.price.into()
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
