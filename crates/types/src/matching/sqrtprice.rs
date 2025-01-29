use std::ops::Deref;

use alloy::primitives::{aliases::U320, Uint, U160, U256};
use malachite::{
    num::{
        arithmetic::traits::{CeilingRoot, DivRound, Pow, PowerOf2},
        conversion::traits::RoundingInto
    },
    Natural, Rational
};
use serde::{Deserialize, Serialize};
use uniswap_v3_math::tick_math::{get_sqrt_ratio_at_tick, get_tick_at_sqrt_ratio};

use super::{const_1e27, const_2_192, Ray};

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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
    use alloy::primitives::{aliases::U320, U160, U256};
    use uniswap_v3_math::tick_math::{get_tick_at_sqrt_ratio, MAX_TICK, MIN_TICK};

    use super::{Ray, SqrtPriceX96};

    #[test]
    fn test_default() {
        let default_price = SqrtPriceX96::default();
        assert_eq!(*default_price, U160::ZERO);
    }

    #[test]
    fn test_as_f64_conversion() {
        // Test small values
        let small_price = SqrtPriceX96::from(U160::from(1_000_000));
        let small_f64 = small_price.as_f64();
        assert!(small_f64 > 0.0);

        // Test larger values
        let large_price = SqrtPriceX96::from(U160::from(u64::MAX));
        let large_f64 = large_price.as_f64();
        assert!(large_f64 > small_f64);

        // Test zero
        let zero_price = SqrtPriceX96::default();
        assert_eq!(zero_price.as_f64(), 0.0);
    }

    #[test]
    fn test_from_float_price() {
        // Test various float values
        let test_prices = [0.0, 1.0, 1.5, 2.0, 10.0, 100.0];

        for price in test_prices {
            let sqrt_price = SqrtPriceX96::from_float_price(price);
            let recovered_price = sqrt_price.as_f64();

            // Allow for some floating point imprecision
            let relative_error = ((recovered_price - price).abs() / price).abs();
            if price != 0.0 {
                assert!(relative_error < 0.01, "Price conversion error too large");
            }
        }
    }

    #[test]
    fn test_tick_conversions() {
        // Test various tick values
        let test_ticks = [-100, -10, 0, 10, 100, 1000, 10000];

        for tick in test_ticks {
            let sqrt_price = SqrtPriceX96::at_tick(tick).unwrap();
            let recovered_tick = sqrt_price.to_tick().unwrap();
            assert_eq!(tick, recovered_tick, "Tick conversion mismatch");
        }

        // Test boundary ticks
        assert!(SqrtPriceX96::at_tick(MIN_TICK - 1).is_err());
        assert!(SqrtPriceX96::at_tick(MAX_TICK + 1).is_err());
        assert!(SqrtPriceX96::at_tick(MIN_TICK).is_ok());
        assert!(SqrtPriceX96::at_tick(MAX_TICK).is_ok());
    }

    #[test]
    fn min_and_max_for_tick() {
        let min_at_tick = SqrtPriceX96::at_tick(100000).unwrap();
        let max_at_tick = SqrtPriceX96::max_at_tick(100000).unwrap();
        let next_tick = SqrtPriceX96::at_tick(100001).unwrap();

        assert!(next_tick != max_at_tick, "Max at tick is equal to next tick");
        assert!(max_at_tick > min_at_tick, "Max not greater than min");
        assert!(
            get_tick_at_sqrt_ratio(max_at_tick.into()).unwrap() == 100000,
            "Max tick outside range"
        );
        assert!(
            get_tick_at_sqrt_ratio(next_tick.into()).unwrap() == 100001,
            "Next tick outside range"
        );
    }

    #[test]
    fn test_as_price_x192() {
        let price = SqrtPriceX96::from(U160::from(1000));
        let price_x192 = price.as_price_x192();
        assert_eq!(price_x192, price.0.widening_mul(price.0));

        // Test zero
        let zero_price = SqrtPriceX96::default();
        assert_eq!(zero_price.as_price_x192(), U320::from(0));
    }

    #[test]
    fn test_from_ray_conversion() {
        // Test zero
        let zero_ray = Ray::default();
        let zero_sqrt = SqrtPriceX96::from(zero_ray);
        assert_eq!(*zero_sqrt, U160::ZERO);

        // Test non-zero values
        let ray = Ray::from(U256::from(1_000_000));
        let sqrt_price = SqrtPriceX96::from(ray);
        assert!(*sqrt_price > U160::ZERO);

        // Test large values
        let large_ray = Ray::from(U256::MAX);
        let large_sqrt = SqrtPriceX96::from(large_ray);
        assert!(*large_sqrt > U160::ZERO);
    }

    #[test]
    fn test_conversions() {
        // Test U160 conversion
        let value = U160::from(12345);
        let price = SqrtPriceX96::from(value);
        assert_eq!(*price, value);

        // Test U256 conversion
        let value_256 = U256::from(12345u64);
        let price = SqrtPriceX96::from(value_256);
        assert_eq!(U256::from(*price), value_256);

        // Test conversion from max values
        let max_160 = U160::MAX;
        let price = SqrtPriceX96::from(max_160);
        assert_eq!(*price, max_160);
    }

    #[test]
    fn test_ordering() {
        let price1 = SqrtPriceX96::from(U160::from(100));
        let price2 = SqrtPriceX96::from(U160::from(200));
        let price3 = SqrtPriceX96::from(U160::from(200));

        assert!(price1 < price2);
        assert!(price2 > price1);
        assert_eq!(price2, price3);
        assert!(price2 >= price3);
        assert!(price2 <= price3);
    }
}
