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

    use super::{MatchingPrice, Ray};
    use crate::matching::SqrtPriceX96;

    #[test]
    fn can_construct_matchingprice() {
        let _ = MatchingPrice::default();
    }

    #[test]
    fn can_convert_ray() {
        let mut rng = thread_rng();
        let value: U256 = rng.sample(rand::distributions::Standard);
        let ray = Ray::from(value);
        let m = MatchingPrice::from(ray);
        assert_eq!(*m, *ray);
    }

    #[test]
    fn can_convert_sqrtpricex96() {
        let mut rng = thread_rng();
        let value: U160 = rng.sample(rand::distributions::Standard);
        let sp96 = SqrtPriceX96::from(value);
        let m = MatchingPrice::from(sp96);
        let ray = Ray::from(sp96);
        assert_eq!(*m, *ray);
    }
}
