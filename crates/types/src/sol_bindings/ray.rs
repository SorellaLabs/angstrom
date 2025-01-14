use std::{
    iter::Sum,
    ops::{Add, AddAssign, Deref, Sub, SubAssign},
    sync::OnceLock
};

use alloy::primitives::{aliases::U320, Uint, U256, U512};
use alloy_primitives::U160;
use malachite::{
    num::{
        arithmetic::traits::{DivRound, Pow},
        conversion::traits::{RoundingInto, SaturatingFrom}
    },
    rounding_modes::RoundingMode,
    Natural, Rational
};
use serde::{Deserialize, Serialize};
use uniswap_v3_math::tick_math::{MAX_SQRT_RATIO, MIN_SQRT_RATIO};

use super::rpc_orders::SolRay;
use crate::matching::{
    const_1e27, const_1e54, const_2_192, uniswap::PoolPrice, MatchingPrice, SqrtPriceX96
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

impl From<SolRay> for Ray {
    fn from(value: SolRay) -> Self {
        Self(value.into())
    }
}

impl Sum for Ray {
    fn sum<I: Iterator<Item = Ray>>(iter: I) -> Self {
        let mut acc = Ray::default();
        for ray in iter {
            acc += ray;
        }
        acc
    }
}

impl From<Ray> for Natural {
    fn from(value: Ray) -> Self {
        Natural::from_limbs_asc(value.0.as_limbs())
    }
}

impl From<Ray> for SolRay {
    fn from(value: Ray) -> Self {
        SolRay::from(value.0)
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

impl<'a> From<PoolPrice<'a>> for Ray {
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

    /// self * ray / other
    pub fn div_ray(mut self, other: Ray) -> Ray {
        self.div_ray_assign(other);
        self
    }

    fn invert(&self, rm: RoundingMode) -> Self {
        let res: Natural = const_1e54().div_round(Natural::from(*self), rm).0;
        Self(U256::from_limbs_slice(&res.to_limbs_asc()))
    }

    /// 1e54 / self
    /// If `round_up` is true, will use RoundingMode::Ceiling, otherwise will
    /// use RoundingMode::Floor.  This is for rounding in the matching engine
    /// where we want to ensure that, depending on the bid/ask nature of the
    /// order, we always round in a direction that is most favorable to us
    pub fn inv_ray_round(&self, round_up: bool) -> Self {
        if round_up {
            self.invert(RoundingMode::Ceiling)
        } else {
            self.invert(RoundingMode::Floor)
        }
    }

    /// 1e54 / self
    pub fn inv_ray_assign(&mut self) {
        *self = self.invert(RoundingMode::Floor);
    }

    /// 1e54 / self
    pub fn inv_ray(self) -> Ray {
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

    pub fn calc_price_generic<T: Into<Natural>>(t0: T, t1: T) -> Self {
        Self::calc_price_inner(t0.into(), t1.into(), RoundingMode::Ceiling)
    }

    fn calc_price_inner(t0: Natural, t1: Natural, rm: RoundingMode) -> Self {
        // P = t1/t0 but we multiply by 1e27 to preserve precision for the Ray format
        let output = (t1 * const_1e27()).div_round(t0, rm).0;
        let inner = U256::from_limbs_slice(&output.into_limbs_asc());
        Self(inner)
    }

    /// Given a price ratio t1/t0 calculates how much t1 would be needed to
    /// output the provided amount of t0 (q) ROUNDED UP
    pub fn mul_quantity(&self, q: U256) -> U256 {
        let p: U512 = self.0.widening_mul(q);
        let numerator = Natural::from_limbs_asc(p.as_limbs());
        let (res, _) =
            numerator.div_round(const_1e27(), malachite::rounding_modes::RoundingMode::Floor);
        let reslimbs = res.into_limbs_asc();
        Uint::from_limbs_slice(&reslimbs)
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
}

#[cfg(test)]
mod tests {
    use alloy::primitives::U160;
    use alloy_primitives::I256;
    use malachite::num::arithmetic::traits::{FloorSqrt, Square};
    use rand::{thread_rng, Rng};
    use uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick;

    use super::*;
    use crate::matching::{const_2_96, debt::Debt};

    #[test]
    fn another_math_test() {
        // AMM Setup
        let liquidity = 1_000_000_000_000_000_000_u128;
        let price_amm_initial = SqrtPriceX96::at_tick(100000).unwrap();
        let price_amm_final = SqrtPriceX96::at_tick(100001).unwrap();
        let delta_0 = uniswap_v3_math::sqrt_price_math::_get_amount_0_delta(
            price_amm_initial.into(),
            price_amm_final.into(),
            liquidity,
            false
        )
        .unwrap();
        println!("Start price:  {:?}\nEnd price: {:?}", price_amm_initial, price_amm_final);
        println!("Delta zero: {}", delta_0);
        let amm_delta = delta_0.to::<u128>() / 4_u128;
        let amount_remaining = I256::unchecked_from(amm_delta) * I256::MINUS_ONE;
        println!("Fraction delta: {:?}\nConverted: {:?}", amm_delta, amount_remaining);
        let step = uniswap_v3_math::swap_math::compute_swap_step(
            price_amm_initial.into(),
            price_amm_final.into(),
            liquidity,
            amount_remaining,
            0
        )
        .unwrap();
        println!("Step {:?}", step);
        println!("Computed final: {:?}", price_amm_final);
        let (end_price, ..) = step;
        let end_sqrt = SqrtPriceX96::from(end_price);
        let final_price = Ray::from(end_sqrt);

        // Debt setup
        let debt_t0_start = U256::from(1_000_000_000_000_u128);
        let debt_fixed_t1: u128 = Ray::from(price_amm_initial)
            .mul_quantity(debt_t0_start)
            .to();
        let debt =
            Debt::new(crate::matching::debt::DebtType::ExactOut(debt_fixed_t1), price_amm_initial);
        println!("Debt start T0: {}\nDebt start T1: {}", debt_t0_start, debt_fixed_t1);
        let calculated_price = Ray::calc_price(debt_t0_start, U256::from(debt_fixed_t1));

        // Prove that our calculated price is as accurate as possible.  The precision is
        // limited by the precision of our input debt numbers which is why it's
        // not perfectly precise. Rounding is going to be a long-term challenge
        // here
        let calculated_price_plus_one =
            Ray::calc_price(U256::from(1_000_000_000_000_u128 + 1_u128), U256::from(debt_fixed_t1));
        let calculated_price_minus_one =
            Ray::calc_price(U256::from(1_000_000_000_000_u128 - 1_u128), U256::from(debt_fixed_t1));
        println!(
            "Start price:  {:?}\nCalc'd Price: {:?}\nCalc'd P+one: {:?}\nCalc'd P-one: {:?}",
            Ray::from(price_amm_initial),
            calculated_price,
            calculated_price_plus_one,
            calculated_price_minus_one
        );
        // Figure out how much t0 we should have at the end price to match
        let debt_t0_final = Ray::from(price_amm_final).inverse_quantity(debt_fixed_t1, true);
        let calculated_price_final =
            Ray::calc_price(U256::from(debt_t0_final), U256::from(debt_fixed_t1));

        // Prove that our calculated final price is as accurate as possible.  Rounding
        // remains a long-term challenge
        let calculated_price_final_plus_one = Ray::calc_price(
            U256::from(debt_t0_final) + U256::from(1_u128),
            U256::from(debt_fixed_t1)
        );
        let calculated_price_final_minus_one = Ray::calc_price(
            U256::from(debt_t0_final) - U256::from(1_u128),
            U256::from(debt_fixed_t1)
        );
        println!(
            "Final price:  {:?}\nCalc'd Price: {:?}\nCalc'd P+one: {:?}\nCalc'd P-one: {:?}",
            Ray::from(price_amm_final),
            calculated_price_final,
            calculated_price_final_plus_one,
            calculated_price_final_minus_one
        );

        // Start doing some math!
        let debt_sqrt_t1_x96 = (Natural::from(debt_fixed_t1) * const_2_192()).floor_sqrt();
        let debt_sqrt_t0_start_x96 =
            (Natural::from_limbs_asc(debt_t0_start.as_limbs()) * const_2_192()).floor_sqrt();
        println!(
            "Token1: {}\tSqrtToken1: {}",
            debt_fixed_t1,
            debt_sqrt_t1_x96
                .clone()
                .div_round(const_2_96(), RoundingMode::Nearest)
                .0
        );

        let a_num_portion_1 = Natural::from(amm_delta) * debt_sqrt_t1_x96.clone();
        println!(
            "----- Num portion 1\nRaw: {}\nReduced: {}\n-----",
            a_num_portion_1,
            a_num_portion_1
                .clone()
                .div_round(const_2_96(), RoundingMode::Nearest)
                .0
        );

        let a_num_portion_2 = Natural::from(liquidity) * debt_sqrt_t0_start_x96.clone();
        println!(
            "----- Num portion 2\nRaw: {}\nReduced: {}\n-----",
            a_num_portion_2,
            a_num_portion_2
                .clone()
                .div_round(const_2_96(), RoundingMode::Nearest)
                .0
        );

        let a_numerator_sum = a_num_portion_2 - a_num_portion_1;
        println!(
            "----- Num sum\nRaw: {}\nReduced: {}\n-----",
            a_numerator_sum,
            a_numerator_sum
                .clone()
                .div_round(const_2_96(), RoundingMode::Nearest)
                .0
        );

        let a_denominator = Natural::from(liquidity);
        let (a_fraction, _) = a_numerator_sum
            .clone()
            .div_round(a_denominator.clone(), RoundingMode::Nearest);
        println!(
            "--- Fraction calc ---\nNumerator: {}\nDenominator: {}\nResult: {}\nRounded result: \
             {}\n--------------------",
            a_numerator_sum,
            a_denominator,
            a_fraction,
            a_fraction
                .clone()
                .div_round(const_2_96(), RoundingMode::Nearest)
                .0
        );

        // if A = sqrt(x + dX) then we have to square A and subtract the original X
        let debt_delta_t0 = Natural::from_limbs_asc(debt_t0_start.as_limbs())
            - a_fraction
                .pow(2)
                .div_round(const_2_192(), RoundingMode::Ceiling)
                .0;
        // let debt_delta_t0_u256 =
        // U256::from_limbs_slice(&debt_delta_t0.to_limbs_asc());

        let debt_delta_t0_u256 = U256::from(debt.calc_proportion(amm_delta, liquidity, true));
        println!("AMM  delta: {:?}", amm_delta);
        println!("Debt delta: {:?}", debt_delta_t0);
        let half_calc_price =
            Ray::calc_price(debt_t0_start - debt_delta_t0_u256, U256::from(debt_fixed_t1));
        let half_plus_one = Ray::calc_price(
            debt_t0_start - debt_delta_t0_u256 + U256::from(1_u128),
            U256::from(debt_fixed_t1)
        );
        let half_minus_one = Ray::calc_price(
            debt_t0_start - debt_delta_t0_u256 - U256::from(1_u128),
            U256::from(debt_fixed_t1)
        );
        println!(
            "BegPrice: {:?}\nHalfCa+1: {:?}\nHalfCalc: {:?}\nHalfCa-1: {:?}\nAMM half: \
             {:?}\nEndPrice: {:?}",
            Ray::from(price_amm_initial),
            half_minus_one,
            half_calc_price,
            half_plus_one,
            final_price,
            Ray::from(price_amm_final)
        );
        println!(
            "Start T0: {}\nFinal T0: {}\nExpec T0: {}\nOur   T0: {}",
            debt_t0_start,
            debt_delta_t0_u256,
            final_price.inverse_quantity(debt_fixed_t1, true),
            half_calc_price.inverse_quantity(debt_fixed_t1, true)
        );
    }

    #[allow(non_snake_case)]
    #[test]
    fn insano_math_test() {
        let l = 1_000_u128;
        let price_amm_initial = get_sqrt_ratio_at_tick(100000).unwrap();
        let price_debt_initial = Ray::from(SqrtPriceX96::at_tick(99999).unwrap());
        let numerator_a = Natural::from(1_u32);
        let denominator_a = Natural::from(l);
        let p_amm = Natural::from_limbs_asc(price_amm_initial.as_limbs());
        let y = Natural::from(l) * p_amm.clone();
        let (x, _): (Natural, _) =
            Rational::from_naturals(Natural::from(l), p_amm).rounding_into(RoundingMode::Ceiling);
        let d1 = Natural::from(1_000_000_000_000_u128);
        let d0 = Natural::from_limbs_asc(
            price_debt_initial
                .mul_quantity(U256::from(1000_u128))
                .as_limbs()
        );

        let a = Rational::from_naturals(numerator_a.clone(), denominator_a.pow(2));
        let b = Rational::from_naturals(numerator_a.clone(), d1.clone())
            - Rational::from_naturals(Natural::from(2_u32), y.clone());
        let c = Rational::from_naturals(d0, d1) - Rational::from_naturals(x, y);
        println!("Made them all!\na: {}\nb: {}\n c:{}", a, b, c);

        let FOUR = Rational::from_naturals(Natural::from(4_u128), numerator_a.clone());
        let TWO = Rational::from_naturals(Natural::from(2_u128), numerator_a.clone());
        println!("bsquared: {}", b.clone().square());
        println!("4ac: {}", (a.clone() * c.clone() * FOUR.clone()));
        let a_squared: Rational = a.clone().pow(2_u64);
        let (v, _): (Natural, _) = (((a.clone() * c * FOUR) - b.clone().square()) * a_squared)
            .rounding_into(RoundingMode::Ceiling);
        println!("v: {}", v);
        let v_sqrt = v.floor_sqrt();
        let nat_b: Natural = b.rounding_into(RoundingMode::Nearest).0;
        println!("V_sqrt: {}\nNat_b: {}", v_sqrt, nat_b);
        let numerator = v_sqrt - nat_b;
        let (denominator, _): (Natural, _) = (TWO * a).rounding_into(RoundingMode::Nearest);
        println!("Numerator: {}\nDenominator: {}", numerator, denominator);
        let solution = Rational::from_naturals(numerator, denominator);
        println!("Kinda found something: {}", solution);
    }

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
