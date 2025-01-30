use std::{
    ops::{Add, Deref},
    sync::OnceLock
};

use alloy::primitives::U256;

mod composite;
pub use composite::CompositeOrder;
pub mod debt;
pub use debt::{Debt, DebtType};
pub mod match_estimate_response;
mod math;
pub use math::max_t1_for_t0;
mod sqrtprice;
mod tokens;
pub mod uniswap;
use malachite::{
    num::{arithmetic::traits::PowerOf2, conversion::traits::FromSciString},
    Natural
};
pub use sqrtprice::SqrtPriceX96;
pub use tokens::TokenQuantity;

pub use super::sol_bindings::Ray;

pub fn const_1e27() -> &'static Natural {
    static TWENTYSEVEN: OnceLock<Natural> = OnceLock::new();
    TWENTYSEVEN.get_or_init(|| Natural::from_sci_string("1e27").unwrap())
}

pub fn const_1e54() -> &'static Natural {
    static FIFTYFOUR: OnceLock<Natural> = OnceLock::new();
    FIFTYFOUR.get_or_init(|| Natural::from_sci_string("1e54").unwrap())
}

pub fn const_2_192() -> &'static Natural {
    static ONENINETWO: OnceLock<Natural> = OnceLock::new();
    ONENINETWO.get_or_init(|| Natural::power_of_2(192))
}

#[allow(unused)]
pub fn const_2_96() -> &'static Natural {
    static ONENINETWO: OnceLock<Natural> = OnceLock::new();
    ONENINETWO.get_or_init(|| Natural::power_of_2(96))
}

pub enum BookSide {
    Bid,
    Ask
}

/// Internal price representation used in the matching engine.
///
/// We'll make sure all the various price representations we work with
/// can be converted to/from this standard so our Math is sane.  This is a Ray.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct MatchingPrice(U256);

impl Deref for MatchingPrice {
    type Target = U256;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Add for MatchingPrice {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl From<SqrtPriceX96> for MatchingPrice {
    fn from(value: SqrtPriceX96) -> Self {
        Ray::from(value).into()
    }
}

impl From<Ray> for MatchingPrice {
    fn from(value: Ray) -> Self {
        Self(*value)
    }
}

impl From<U256> for MatchingPrice {
    fn from(value: U256) -> Self {
        Self(value)
    }
}

#[cfg(test)]
mod tests {
    use alloy::primitives::{U160, U256};
    use rand::{thread_rng, Rng};
    use uniswap_v3_math::tick_math::MIN_SQRT_RATIO;

    use super::{MatchingPrice, Ray};
    use crate::matching::SqrtPriceX96;

    #[test]
    fn test_matching_price_default() {
        let default_price = MatchingPrice::default();
        assert_eq!(*default_price, U256::ZERO);
    }

    #[test]
    fn test_matching_price_non_default() {
        let non_default = MatchingPrice(U256::from(1));
        assert_ne!(*non_default, U256::ZERO);

        let max_value = MatchingPrice(U256::MAX);
        assert_ne!(*max_value, U256::ZERO);
    }

    #[test]
    fn test_matching_price_deref() {
        let value = U256::from(12345);
        let price = MatchingPrice(value);
        assert_eq!(*price, value);

        // Test deref with different values
        let values = [U256::from(0), U256::from(1), U256::MAX, U256::from(1) << 128];

        for val in values {
            let price = MatchingPrice(val);
            assert_eq!(*price, val);
        }
    }

    #[test]
    fn test_matching_price_add() {
        let price1 = MatchingPrice(U256::from(100));
        let price2 = MatchingPrice(U256::from(200));
        let sum = price1 + price2;
        assert_eq!(*sum, U256::from(300));

        // Test addition with zero
        let price = MatchingPrice(U256::from(100));
        let zero = MatchingPrice(U256::ZERO);
        assert_eq!((price.clone() + zero), price);

        // Test addition near max values
        let max = MatchingPrice(U256::MAX);
        let one = MatchingPrice(U256::from(1));
        let result = max + one;
        assert_eq!(*result, U256::from(0)); // Should wrap around
    }

    #[test]
    fn test_matching_price_from_ray() {
        // Test conversion with zero
        let zero_ray = Ray::from(U256::ZERO);
        let zero_price = MatchingPrice::from(zero_ray);
        assert_eq!(*zero_price, U256::ZERO);

        // Test conversion with max value
        let max_ray = Ray::from(U256::MAX);
        let max_price = MatchingPrice::from(max_ray);
        assert_eq!(*max_price, U256::MAX);

        // Test random conversions
        let mut rng = thread_rng();
        for _ in 0..10 {
            let value: U256 = rng.sample(rand::distributions::Standard);
            let ray = Ray::from(value);
            let price = MatchingPrice::from(ray);
            assert_eq!(*price, *ray);
        }
    }

    #[test]
    fn test_matching_price_from_sqrtpricex96() {
        // Test conversion with minimum valid value
        let min_value = MIN_SQRT_RATIO;
        let min_sp96 = SqrtPriceX96::from(min_value);
        let min_price = MatchingPrice::from(min_sp96);
        assert_eq!(*min_price, U256::ZERO);

        // Test conversion with maximum valid value
        let max_value = U160::MAX;
        let max_sp96 = SqrtPriceX96::from(max_value);
        let max_price = MatchingPrice::from(max_sp96);
        assert_ne!(*max_price, U256::ZERO);

        // Test random conversions and verify consistency
        let mut rng = thread_rng();
        for _ in 0..10 {
            let value: U160 = rng.sample(rand::distributions::Standard);
            let sp96 = SqrtPriceX96::from(value);
            let price = MatchingPrice::from(sp96);
            let ray = Ray::from(sp96);
            assert_eq!(*price, *ray);
            assert_ne!(*price, U256::ZERO);
        }
    }

    #[test]
    fn test_matching_price_from_u256() {
        // Test conversion with zero
        let zero_price = MatchingPrice::from(U256::ZERO);
        assert_eq!(*zero_price, U256::ZERO);

        // Test conversion with max value
        let max_price = MatchingPrice::from(U256::MAX);
        assert_eq!(*max_price, U256::MAX);

        // Test conversion with various values
        let test_values =
            [U256::from(1), U256::from(1000), U256::from(1) << 128, U256::MAX - U256::from(1)];

        for value in test_values {
            let price = MatchingPrice::from(value);
            assert_eq!(*price, value);
        }
    }

    #[test]
    fn test_matching_price_ordering() {
        let price1 = MatchingPrice(U256::from(100));
        let price2 = MatchingPrice(U256::from(200));
        let price3 = MatchingPrice(U256::from(200));

        // Test PartialOrd
        assert!(price1 < price2);
        assert!(price2 > price1);
        assert!(price2 >= price3);
        assert!(price2 <= price3);

        // Test Ord
        assert_eq!(price2.cmp(&price3), std::cmp::Ordering::Equal);
        assert_eq!(price1.cmp(&price2), std::cmp::Ordering::Less);
        assert_eq!(price2.cmp(&price1), std::cmp::Ordering::Greater);
    }
}
