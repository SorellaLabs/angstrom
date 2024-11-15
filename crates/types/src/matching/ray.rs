use std::ops::{Add, AddAssign, Deref, Mul, Sub, SubAssign};

use alloy::primitives::{aliases::U320, Uint, U256, U512};
use malachite::{
    num::{
        arithmetic::traits::{DivRound, Pow},
        conversion::traits::RoundingInto
    },
    rounding_modes::RoundingMode,
    Natural, Rational
};
use serde::{Deserialize, Serialize};

use super::{const_2_192, MatchingPrice, SqrtPriceX96};
use crate::matching::const_1e27;
#[derive(Copy, Clone, Debug, Default, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ray(U256);

impl Deref for Ray {
    type Target = U256;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Sub for Ray {
    type Output = Ray;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl SubAssign for Ray {
    fn sub_assign(&mut self, rhs: Self) {
        *self = Self(self.0 - rhs.0)
    }
}

impl Add for Ray {
    type Output = Ray;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl Add<usize> for Ray {
    type Output = Ray;

    fn add(self, rhs: usize) -> Self::Output {
        Self(self.0 + Uint::from(rhs))
    }
}

impl AddAssign for Ray {
    fn add_assign(&mut self, rhs: Self) {
        *self = Self(self.0 + rhs.0);
    }
}
impl Mul for Ray {
    type Output = Ray;

    #[allow(clippy::suspicious_arithmetic_impl)]
    fn mul(self, rhs: Self) -> Self::Output {
        let mult: U512 = self.widening_mul(rhs.0);
        // div by 1e27
        let out = (mult / U512::from(1e27)).into_limbs();

        let buf = [out[4], out[5], out[6], out[7]];
        Ray::from(U256::from_limbs(buf))
    }
}

impl From<U256> for Ray {
    fn from(value: U256) -> Self {
        Self(value)
    }
}

impl From<Ray> for U256 {
    fn from(value: Ray) -> Self {
        value.0
    }
}

impl From<u8> for Ray {
    fn from(value: u8) -> Self {
        Self(Uint::from(value))
    }
}

impl From<usize> for Ray {
    fn from(value: usize) -> Self {
        Self(Uint::from(value))
    }
}

// Can't do this because of something in the blsful crate, annoying!
// trait RayConvert {}

// impl<T: Into<U256> + RayConvert> From<T> for Ray {
//     fn from(value: T) -> Self {
//         Self(Uint::from(value))
//     }
// }

impl From<f64> for Ray {
    fn from(value: f64) -> Self {
        Self(U256::from((value * (10.0_f64.pow(27))).floor()))
    }
}

impl From<&Ray> for f64 {
    fn from(value: &Ray) -> Self {
        let numerator = Natural::from_limbs_asc(value.0.as_limbs());
        let denominator: Natural = const_1e27().clone();
        let price = Rational::from_naturals(numerator, denominator);
        let (res, _) = price.rounding_into(malachite::rounding_modes::RoundingMode::Floor);
        res
    }
}

impl From<SqrtPriceX96> for Ray {
    fn from(value: SqrtPriceX96) -> Self {
        let p: U320 = value.widening_mul(*value);

        let numerator = Natural::from_limbs_asc(p.as_limbs()) * const_1e27();
        let (res, _) =
            numerator.div_round(const_2_192(), malachite::rounding_modes::RoundingMode::Floor);
        let reslimbs = res.into_limbs_asc();
        let output: U256 = Uint::from_limbs_slice(&reslimbs);
        Self(output)
    }
}

impl From<MatchingPrice> for Ray {
    fn from(value: MatchingPrice) -> Self {
        Self(*value)
    }
}

impl Serialize for Ray {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Ray {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>
    {
        let inner = U256::deserialize(deserializer)?;
        Ok(Self(inner))
    }
}

impl Ray {
    pub const ZERO: Ray = Ray(U256::ZERO);

    /// Uses malachite.rs to approximate this value as a floating point number.
    /// Converts from the internal U256 representation to an approximated f64
    /// representation, which is a change to the value of this number and why
    /// this isn't `From<Ray> for f64`
    pub fn as_f64(&self) -> f64 {
        self.into()
    }

    /// Calculates a price ratio t1/t0
    pub fn calc_price(t0: U256, t1: U256) -> Self {
        let numerator = Natural::from_limbs_asc(t1.as_limbs()) * const_1e27();
        let denominator = Natural::from_limbs_asc(t0.as_limbs());
        let output = Rational::from_naturals(numerator, denominator);
        let (natout, _): (Natural, _) = output.rounding_into(RoundingMode::Floor);
        let limbs = natout.limbs().collect::<Vec<_>>();
        let inner = U256::from_limbs_slice(&limbs);
        Self(inner)
    }

    /// Given a price ratio t1/t0 calculates how much t1 would be needed to
    /// output the provided amount of t0 (q)
    pub fn mul_quantity(&self, q: U256) -> U256 {
        let p: U512 = self.0.widening_mul(q);
        let numerator = Natural::from_limbs_asc(p.as_limbs());
        let (res, _) =
            numerator.div_round(const_1e27(), malachite::rounding_modes::RoundingMode::Ceiling);
        let reslimbs = res.into_limbs_asc();
        Uint::from_limbs_slice(&reslimbs)
    }

    /// Given a price ratio t1/t0 calculates how much t0 would be needed to
    /// output the provided amount of t1 (q)
    pub fn inverse_quantity(&self, q: U256) -> U256 {
        let numerator = Natural::from_limbs_asc(q.as_limbs()) * const_1e27();
        let denominator = Natural::from_limbs_asc(self.0.as_limbs());
        let output = Rational::from_naturals(numerator, denominator);
        let (natout, _): (Natural, _) = output.rounding_into(RoundingMode::Ceiling);
        U256::from_limbs_slice(&natout.to_limbs_asc())
    }
}

#[cfg(test)]
mod tests {
    use alloy::primitives::U160;
    use rand::{thread_rng, Rng};

    use super::*;

    #[test]
    fn converts_to_and_from_f64() {
        let test_val: f64 = 123456.1234567899;
        let ray = Ray::from(test_val);
        let ray_float = ray.as_f64();
        assert_eq!(test_val, ray_float, "Ray float not equal to original float");

        // let tgt_uint: U256 =
        // Uint::from(123456123456789900000000000000000_u128);
        // Looks like we need to make some decisions about how close to a float
        // we're willing to get
        // assert_eq!(
        //     ray.0, tgt_uint,
        //     "Ray does not properly equal target
        // uint"
        // );
    }

    #[test]
    fn converts_from_sqrtpricex96() {
        let mut rng = thread_rng();
        // Make sure our random number fits in here
        let x: U160 = rng.sample(rand::distributions::Standard);
        // let random: U256 = U256::ZERO;
        // let sp = Ray(random);
        let sp: SqrtPriceX96 = Ray(Uint::from(x)).into();

        let rp: Ray = sp.into();
        let sptwo: SqrtPriceX96 = rp.into();
        let rptwo: Ray = sptwo.into();
        let spthree: SqrtPriceX96 = rptwo.into();
        let rpthree: Ray = spthree.into();

        println!("{:?} - {:?} - {:?}", rp, rptwo, rpthree);
        println!("{:?} - {:?} - {:?}", sp, sptwo, spthree);
        println!("{} - {}", rp.as_f64(), sp.as_f64());
        assert!(rp.as_f64() == sp.as_f64());
        assert!(rp == rptwo);
        assert!(rp == rpthree);
        assert!(sp == sptwo);
        assert!(sp == spthree);
    }
}
