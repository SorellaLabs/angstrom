use std::{
    iter::Sum,
    ops::{Add, AddAssign, Deref, Sub, SubAssign},
    sync::OnceLock
};

use alloy::primitives::{U256, U512, Uint, aliases::U320};
use alloy_primitives::U160;
use malachite::{
    Natural, Rational,
    num::{
        arithmetic::traits::{CeilingRoot, DivRound, Mod, Pow, SaturatingSub},
        conversion::traits::{RoundingInto, SaturatingFrom}
    },
    rounding_modes::RoundingMode
};
use serde::{Deserialize, Serialize};
use uniswap_v3_math::tick_math::{MAX_SQRT_RATIO, MIN_SQRT_RATIO};

use crate::matching::{
    MatchingPrice, SqrtPriceX96, const_1e6, const_1e27, const_1e54, const_2_192,
    uniswap::{PoolPrice, Quantity}
};

fn max_tick_ray() -> &'static Ray {
    static MAX_TICK_PRICE: OnceLock<Ray> = OnceLock::new();
    MAX_TICK_PRICE.get_or_init(|| Ray::from(SqrtPriceX96::from(MAX_SQRT_RATIO)))
}

fn min_tick_ray() -> &'static Ray {
    static MIN_TICK_PRICE: OnceLock<Ray> = OnceLock::new();
    MIN_TICK_PRICE.get_or_init(|| Ray::from(SqrtPriceX96::from(MIN_SQRT_RATIO)))
}
#[derive(Copy, Clone, Debug, Default, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ray(pub U256);

impl Sum for Ray {
    fn sum<I: Iterator<Item = Ray>>(iter: I) -> Self {
        let mut acc = Ray::default();
        for ray in iter {
            acc += ray;
        }
        acc
    }
}

impl PartialEq<U256> for Ray {
    fn eq(&self, other: &U256) -> bool {
        self.0.eq(other)
    }
}

impl PartialOrd<U256> for Ray {
    fn partial_cmp(&self, other: &U256) -> Option<std::cmp::Ordering> {
        self.0.partial_cmp(other)
    }
}

impl From<Ray> for Natural {
    fn from(value: Ray) -> Self {
        Natural::from_limbs_asc(value.0.as_limbs())
    }
}

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

impl Sub<usize> for Ray {
    type Output = Ray;

    fn sub(self, rhs: usize) -> Self::Output {
        Self(self.0.saturating_sub(Uint::from(rhs)))
    }
}

impl SubAssign for Ray {
    fn sub_assign(&mut self, rhs: Self) {
        *self = Self(self.0 - rhs.0)
    }
}

impl std::ops::Mul<U256> for Ray {
    type Output = Ray;

    fn mul(self, rhs: U256) -> Self::Output {
        Ray::from(self.0 * rhs)
    }
}

impl std::ops::Div<U256> for Ray {
    type Output = Ray;

    fn div(self, rhs: U256) -> Self::Output {
        Ray::from(self.0 / rhs)
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

impl From<U256> for Ray {
    fn from(value: U256) -> Self {
        Self(value)
    }
}

impl From<u128> for Ray {
    fn from(value: u128) -> Self {
        Self(U256::from(value))
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

impl From<f64> for Ray {
    fn from(value: f64) -> Self {
        Self(U256::from((value * (10.0_f64.pow(27))).floor()))
    }
}

impl From<&Ray> for f64 {
    fn from(value: &Ray) -> Self {
        let numerator = Natural::from_limbs_asc(value.0.as_limbs());
        let price = Rational::from_naturals(numerator, const_1e27().clone());
        price
            .rounding_into(malachite::rounding_modes::RoundingMode::Floor)
            .0
    }
}

/// Local utility function for doing the math needed to convert a SqrtPriceX96
/// into our Ray format, we use this in a few places so it's written only once
/// here
fn convert_sqrtpricex96(price: &U160, round_up: bool) -> Ray {
    let p: U320 = price.widening_mul(*price);
    let rm = if round_up { RoundingMode::Ceiling } else { RoundingMode::Floor };
    let numerator = Natural::from_limbs_asc(p.as_limbs()) * const_1e27();
    let (res, _) = numerator.div_round(const_2_192(), rm);
    Ray(U256::from_limbs_slice(&res.into_limbs_asc()))
}

impl From<&SqrtPriceX96> for Ray {
    fn from(price: &SqrtPriceX96) -> Self {
        convert_sqrtpricex96(price, false)
    }
}

impl From<SqrtPriceX96> for Ray {
    fn from(price: SqrtPriceX96) -> Self {
        convert_sqrtpricex96(&price, false)
    }
}

impl From<&MatchingPrice> for Ray {
    fn from(value: &MatchingPrice) -> Self {
        Self(**value)
    }
}

impl From<MatchingPrice> for Ray {
    fn from(value: MatchingPrice) -> Self {
        Self(*value)
    }
}

impl From<PoolPrice<'_>> for Ray {
    fn from(value: PoolPrice) -> Self {
        Self::from(value.price)
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

    /// checks to assert that when this price is converted to a SqrtPriceX96
    /// that we will never overflow or underflow
    pub fn within_sqrt_price_bounds(&self) -> bool {
        let numerator = Natural::from_limbs_asc(self.as_limbs()) * const_2_192();
        let (res, _) =
            numerator.div_round(const_1e27(), malachite::rounding_modes::RoundingMode::Ceiling);
        let root = res.ceiling_root(2);
        let reslimbs = root.into_limbs_asc();
        let output: U256 = Uint::from_limbs_slice(&reslimbs);

        MIN_SQRT_RATIO < output && MAX_SQRT_RATIO > output
    }

    /// given a value and a decimal point, generates a ray
    /// ex value: 100 and decimal_point = 1 -> ray value of 10.0
    pub fn generate_ray_decimal(value: u128, decimal_point: u8) -> Ray {
        let ray_precision = 27 - decimal_point as u64;
        let value = Natural::from(value) * Natural::from(10u128).pow(ray_precision);

        Ray::from(Uint::from_limbs_slice(&value.to_limbs_asc()))
    }

    /// value * 1e27
    pub fn scale_to_ray(value: U256) -> Ray {
        let value = Natural::from_limbs_asc(value.as_limbs()) * const_1e27();

        Ray::from(Uint::from_limbs_slice(&value.to_limbs_asc()))
    }

    /// value / 1e27
    pub fn scale_out_of_ray(self) -> U256 {
        let numerator = Natural::from_limbs_asc(self.0.as_limbs());
        let denominator: Natural = const_1e27().clone();
        let price = Rational::from_naturals(numerator, denominator);
        let (res, _): (Natural, _) =
            price.rounding_into(malachite::rounding_modes::RoundingMode::Floor);

        Uint::from_limbs_slice(&res.into_limbs_asc())
    }

    /// Produce a price that, post fee scaling, will equal this price
    pub fn unscale_to_fee(&self, fee: u128) -> Ray {
        if fee == 0 {
            return *self;
        }
        let numerator = Natural::from_limbs_asc(self.0.as_limbs()) * const_1e6();
        let one_minus_fee = const_1e6().saturating_sub(Natural::from(fee));
        let res = numerator.div_round(one_minus_fee, RoundingMode::Floor).0;
        Self(Uint::from_limbs_slice(&res.into_limbs_asc()))
    }

    /// Scale this price to a given fee
    pub fn scale_to_fee(&self, fee: u128) -> Ray {
        // Short circuit if we have a zero fee, no need to do math
        if fee == 0 {
            return *self;
        }
        let numerator = Natural::from_limbs_asc(self.0.as_limbs())
            * const_1e6().saturating_sub(Natural::from(fee));
        let res = numerator.div_round(const_1e6(), RoundingMode::Floor).0;
        Self(Uint::from_limbs_slice(&res.into_limbs_asc()))
    }

    /// self * other / ray
    pub fn mul_ray_assign(&mut self, other: Ray) {
        let p: U512 = self.0.widening_mul(other.0);
        let numerator = Natural::from_limbs_asc(p.as_limbs());
        let (res, _) =
            numerator.div_round(const_1e27(), malachite::rounding_modes::RoundingMode::Floor);
        let reslimbs = res.into_limbs_asc();

        *self = Ray::from(Uint::from_limbs_slice(&reslimbs));
    }

    /// self * other / ray
    pub fn mul_ray(mut self, other: Ray) -> Ray {
        self.mul_ray_assign(other);
        self
    }

    /// self * ray / other
    pub fn div_ray_assign(&mut self, other: Ray) {
        let numerator = Natural::from_limbs_asc(self.0.as_limbs());
        let num = numerator * const_1e27();

        let denom = Natural::from_limbs_asc(other.0.as_limbs());
        let res = Rational::from_naturals(num, denom);
        let (n, _): (Natural, _) = res.rounding_into(RoundingMode::Floor);
        let this = U256::from_limbs_slice(&n.to_limbs_asc());

        *self = Ray::from(this);
    }

    fn invert(&self, rm: RoundingMode) -> Self {
        let (res, _) = const_1e54().div_round(Natural::from(*self), rm);
        Self(U256::from_limbs_slice(&res.to_limbs_asc()))
    }

    /// 1e54 / self
    /// If `round_up` is true, will use RoundingMode::Ceiling, otherwise will
    /// use RoundingMode::Floor.  This is for rounding in the matching engine
    /// where we want to ensure that, depending on the bid/ask nature of the
    /// order, we always round in a direction that is most favorable to us
    pub fn inv_ray_round(&self, round_up: bool) -> Ray {
        if round_up { self.invert(RoundingMode::Ceiling) } else { self.invert(RoundingMode::Floor) }
    }

    pub fn mul_wad<T: Into<Natural>>(&self, mul: T, decimals: u8) -> Self {
        let mul_val: Natural = mul.into();
        let decimals = Natural::from(10u128).pow(decimals as u64);
        let numerator = Natural::from_limbs_asc(self.0.as_limbs());

        let num = numerator * mul_val;
        let res = Rational::from_naturals(num, decimals);
        let (n, _): (Natural, _) = res.rounding_into(RoundingMode::Floor);

        let this = U256::from_limbs_slice(&n.to_limbs_asc());

        Ray::from(this)
    }

    pub fn div_wad<T: Into<Natural>>(&self, div: T, decimals: u8) -> Self {
        let div_val: Natural = div.into();
        let decimals = Natural::from(10u128).pow(decimals as u64);
        let numerator = Natural::from_limbs_asc(self.0.as_limbs());

        let num = numerator * decimals;
        let res = Rational::from_naturals(num, div_val);
        let (n, _): (Natural, _) = res.rounding_into(RoundingMode::Floor);

        let this = U256::from_limbs_slice(&n.to_limbs_asc());

        Ray::from(this)
    }

    /// 1e54 / self
    pub fn inv_ray_assign(&mut self) {
        *self = self.invert(RoundingMode::Floor);
    }

    pub fn inv_ray_assign_round(&mut self, round_up: bool) {
        *self = self.inv_ray_round(round_up);
    }

    /// 1e54 / self
    pub fn inv_ray(self) -> Ray {
        if self.is_zero() {
            return self;
        }
        self.invert(RoundingMode::Floor)
    }

    pub fn max_uniswap_price() -> Self {
        *max_tick_ray()
    }

    pub fn min_uniswap_price() -> Self {
        *min_tick_ray()
    }

    /// Uses malachite.rs to approximate this value as a floating point number.
    /// Converts from the internal U256 representation to an approximated f64
    /// representation, which is a change to the value of this number and why
    /// this isn't `From<Ray> for f64`
    pub fn as_f64(&self) -> f64 {
        self.into()
    }

    /// Calculates a price ratio t1/t0
    pub fn calc_price(t0: U256, t1: U256) -> Self {
        let t0 = Natural::from_limbs_asc(t0.as_limbs());
        let t1 = Natural::from_limbs_asc(t1.as_limbs());
        Self::calc_price_inner(t0, t1, RoundingMode::Ceiling)
    }

    pub fn calc_price_generic<T: Into<Natural>>(t0: T, t1: T, round_up: bool) -> Self {
        let rm = if round_up { RoundingMode::Ceiling } else { RoundingMode::Floor };
        Self::calc_price_inner(t0.into(), t1.into(), rm)
    }

    fn calc_price_inner(t0: Natural, t1: Natural, rm: RoundingMode) -> Self {
        // P = t1/t0 but we multiply by 1e27 to preserve precision for the Ray format
        let output = (t1 * const_1e27()).div_round(t0, rm).0;
        let inner = U256::from_limbs_slice(&output.into_limbs_asc());
        Self(inner)
    }

    /// Given a quantity, determine the cost of that quantity at the current
    /// price
    pub fn price_of(&self, q: Quantity, round_up: bool) -> u128 {
        match q {
            Quantity::Token0(t0) => self.quantity(t0, round_up),
            Quantity::Token1(t1) => self.inverse_quantity(t1, round_up)
        }
    }

    /// Given a price ratio t1/t0 calculates how much t1 would be needed to
    /// output the provided amount of t0 (q) rounds DOWN by default
    pub fn mul_quantity(&self, q: U256) -> U256 {
        let p: U512 = self.0.widening_mul(q);
        let numerator = Natural::from_limbs_asc(p.as_limbs());
        let (res, _) =
            numerator.div_round(const_1e27(), malachite::rounding_modes::RoundingMode::Floor);
        let reslimbs = res.into_limbs_asc();
        Uint::from_limbs_slice(&reslimbs)
    }

    /// Given a price ration t1/t0 calculates how much t1 would be needed to
    /// output the provided amount of t0 (q).  Rounding determined by parameter
    pub fn quantity(&self, q: u128, round_up: bool) -> u128 {
        let rm = if round_up { RoundingMode::Ceiling } else { RoundingMode::Floor };
        let numerator = Natural::from_limbs_asc(self.0.as_limbs()) * Natural::from(q);
        let (res, _) = numerator.div_round(const_1e27(), rm);
        u128::saturating_from(&res)
    }

    /// Given a price ratio t1/t0 calculates how much t0 would be needed to
    /// output the provided amount of t1 (q).  Rounding determined by parameter
    pub fn inverse_quantity(&self, q: u128, round_up: bool) -> u128 {
        let rm = if round_up { RoundingMode::Ceiling } else { RoundingMode::Floor };
        let numerator = Natural::from(q) * const_1e27();
        let denominator = Natural::from_limbs_asc(self.0.as_limbs());
        let (res, _) = numerator.div_round(denominator, rm);
        u128::saturating_from(&res)
    }

    /// Given a price ratio t1/t0 calculates the amount of excess T1 left after
    /// dividing out an even amount of T0
    pub fn inverse_remainder(&self, q: u128) -> u128 {
        let numerator = Natural::from(q) * const_1e27();
        let denominator = Natural::from_limbs_asc(self.0.as_limbs());
        let remainder = numerator.mod_op(denominator);
        u128::saturating_from(&remainder)
    }

    /// Assumes that amounts are correctly scaled to decimals
    pub fn from_quantities(amount_out: u128, amount_in: u128, round_up: bool) -> Ray {
        let rm = if round_up { RoundingMode::Ceiling } else { RoundingMode::Floor };
        let numerator = Natural::from(amount_out) * const_1e27();
        let denominator = Natural::from(amount_in);

        let (res, _) = numerator.div_round(denominator, rm);
        let inner = U256::from_limbs_slice(&res.into_limbs_asc());

        Self(inner)
    }
}

#[cfg(test)]
mod tests {
    use alloy::primitives::U160;
    use rand::{Rng, rng};

    use super::*;

    #[test]
    fn from_quantities() {
        // swapping from 1eth to 3500 usdc
        let amount_out = (3500.0 * 10.0.pow(6i64)) as u128;
        let amount_in = (10.0.pow(18i64)) as u128;

        let price = Ray::from_quantities(amount_out, amount_in, false);
        println!("{price:?}");

        // test 1 way
        let new_amount_out = price.quantity(amount_in, false);
        assert_eq!(
            new_amount_out, amount_out,
            "new_am: {new_amount_out} original_am: {amount_out}"
        );

        // test other way
        let new_amount_in = price.inverse_quantity(amount_out, false);
        assert_eq!(new_amount_in, amount_in, "new_am: {new_amount_in} original_am: {amount_in}")
    }

    #[test]
    fn converts_to_and_from_f64() {
        let test_val: f64 = 123456.1234567899;
        let ray = Ray::from(test_val);
        let ray_float = ray.as_f64();
        assert_eq!(test_val, ray_float, "Ray float not equal to original float");
    }

    #[test]
    fn converts_from_sqrtpricex96() {
        let mut rng = rng();
        // Make sure our random number fits in here
        let x: U160 = rng.sample(rand::distr::StandardUniform);
        // let random: U256 = U256::ZERO;
        // let sp = Ray(random);
        let sp: SqrtPriceX96 = Ray(Uint::from(x)).into();

        let rp: Ray = sp.into();
        let sptwo: SqrtPriceX96 = rp.into();
        let rptwo: Ray = sptwo.into();
        let spthree: SqrtPriceX96 = rptwo.into();
        let rpthree: Ray = spthree.into();

        println!("{rp:?} - {rptwo:?} - {rpthree:?}");
        println!("{sp:?} - {sptwo:?} - {spthree:?}");
        println!("{} - {}", rp.as_f64(), sp.as_f64());
        assert!(rp.as_f64() == sp.as_f64());
        assert!(rp == rptwo);
        assert!(rp == rpthree);
        assert!(sp == sptwo);
        assert!(sp == spthree);
    }
}
