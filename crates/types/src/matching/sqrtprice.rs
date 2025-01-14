use std::ops::Deref;

use alloy::primitives::{aliases::U320, Uint, U160, U256};
use malachite::{
    num::{
        arithmetic::traits::{CeilingRoot, DivRound, Pow, PowerOf2},
        conversion::traits::RoundingInto
    },
    Natural, Rational
};
use uniswap_v3_math::tick_math::{get_sqrt_ratio_at_tick, get_tick_at_sqrt_ratio};

use super::{const_1e27, const_2_192, Ray};

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SqrtPriceX96(U160);

impl SqrtPriceX96 {
    /// Uses malachite.rs to approximate this value as a floating point number.
    /// Converts from the internal U160 representation of `sqrt(P)` to an
    /// approximated f64 representation of `P`, which is a change to the
    /// value of this number and why this isn't `From<SqrtPriceX96> for f64`
    pub fn as_f64(&self) -> f64 {
        let numerator = Natural::from_limbs_asc(self.0.as_limbs());
        let denominator: Natural = Natural::power_of_2(96u64);
        let sqrt_price = Rational::from_naturals(numerator, denominator);
        let price = sqrt_price.pow(2u64);
        let (res, _) = price.rounding_into(malachite::rounding_modes::RoundingMode::Floor);
        res
    }

    /// Convert a floating point price `P` into a SqrtPriceX96 `sqrt(P)`
    pub fn from_float_price(price: f64) -> Self {
        SqrtPriceX96(U160::from(price.sqrt() * (2.0_f64.pow(96))))
    }

    /// Produces the SqrtPriceX96 precisely at a given tick
    pub fn at_tick(tick: i32) -> eyre::Result<Self> {
        Ok(Self::from(get_sqrt_ratio_at_tick(tick)?))
    }

    /// Produces the maximum SqrtPriceX96 valid for a given tick before we step
    /// forward into the next tick
    pub fn max_at_tick(tick: i32) -> eyre::Result<Self> {
        Ok(Self::from(get_sqrt_ratio_at_tick(tick + 1)?.saturating_sub(U256::from(1))))
    }

    pub fn to_tick(&self) -> eyre::Result<i32> {
        Ok(get_tick_at_sqrt_ratio(U256::from(self.0))?)
    }

    /// Squares this value with no loss of precision, returning a U320 that
    /// contains PriceX192
    pub fn as_price_x192(&self) -> U320 {
        self.0.widening_mul(self.0)
    }
}

impl Deref for SqrtPriceX96 {
    type Target = U160;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<SqrtPriceX96> for U256 {
    fn from(value: SqrtPriceX96) -> Self {
        Uint::from(value.0)
    }
}

impl From<U256> for SqrtPriceX96 {
    fn from(value: U256) -> Self {
        Self(Uint::from(value))
    }
}

impl From<U160> for SqrtPriceX96 {
    fn from(value: U160) -> Self {
        Self(value)
    }
}

impl From<Ray> for SqrtPriceX96 {
    fn from(value: Ray) -> Self {
        let numerator = Natural::from_limbs_asc(value.as_limbs()) * const_2_192();
        let (res, _) =
            numerator.div_round(const_1e27(), malachite::rounding_modes::RoundingMode::Ceiling);
        let root = res.ceiling_root(2);
        let reslimbs = root.into_limbs_asc();
        let output: U160 = Uint::from_limbs_slice(&reslimbs);
        Self(output)
    }
}

#[cfg(test)]
mod tests {
    use uniswap_v3_math::tick_math::get_tick_at_sqrt_ratio;

    use super::SqrtPriceX96;

    #[test]
    fn min_and_max_for_tick() {
        let _min_at_tick = SqrtPriceX96::at_tick(100000).unwrap();
        let max_at_tick = SqrtPriceX96::max_at_tick(100000).unwrap();
        let next_tick = SqrtPriceX96::at_tick(100001).unwrap();

        assert!(next_tick != max_at_tick, "Max at tick is equal to next tick");
        assert!(
            get_tick_at_sqrt_ratio(max_at_tick.into()).unwrap() == 100000,
            "Max tick outside range"
        );
        assert!(
            get_tick_at_sqrt_ratio(next_tick.into()).unwrap() == 100001,
            "Next tick outside range"
        );
    }
}
