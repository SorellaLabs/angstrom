use std::ops::{Add, AddAssign, Sub};

use malachite::{
    num::{
        arithmetic::traits::{DivRound, FloorSqrt, Pow},
        conversion::traits::SaturatingFrom
    },
    rounding_modes::RoundingMode,
    Natural
};

use super::{math::low_to_high, uniswap::PoolPrice, Ray};
use crate::matching::const_2_192;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DebtType {
    ExactIn(u128),
    ExactOut(u128)
}

impl DebtType {
    pub fn exact_in(q: u128) -> Self {
        Self::ExactIn(q)
    }

    pub fn exact_out(q: u128) -> Self {
        Self::ExactOut(q)
    }

    pub fn same_type(&self, q: u128) -> Self {
        match self {
            Self::ExactIn(_) => Self::ExactIn(q),
            Self::ExactOut(_) => Self::ExactOut(q)
        }
    }

    pub fn magnitude(&self) -> u128 {
        match self {
            Self::ExactIn(q) | Self::ExactOut(q) => *q
        }
    }

    pub fn is_empty(&self) -> bool {
        self.magnitude() == 0
    }

    pub fn t0_at_price<T: Into<Ray>>(&self, price: T) -> u128 {
        // If it's an ExactIn debt our output is rounded down, otherwise it's ExactOut
        // and the input is rounded up
        let round_up = match self {
            Self::ExactIn(_) => false,
            Self::ExactOut(_) => true
        };
        let ray_price: Ray = price.into();
        ray_price.inverse_quantity(self.magnitude(), round_up)
    }

    pub fn slack_at_price<T: Into<Ray>>(&self, price: T) -> u128 {
        let ray_price: Ray = price.into();
        ray_price.inverse_remainder(self.magnitude())
    }

    pub fn same_side(&self, other: &Self) -> bool {
        if let Self::ExactIn(_) = self {
            matches!(other, Self::ExactIn(_))
        } else {
            matches!(other, Self::ExactOut(_))
        }
    }
}

impl Add for DebtType {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            // Like types accumulate
            (Self::ExactIn(q_1), Self::ExactIn(q_2)) => Self::ExactIn(q_1 + q_2),
            (Self::ExactOut(q_1), Self::ExactOut(q_2)) => Self::ExactOut(q_1 + q_2),
            // Different types annihilate or maybe flip
            (Self::ExactIn(q_1), Self::ExactOut(q_2)) if q_2 > q_1 => Self::ExactOut(q_2 - q_1),
            (Self::ExactIn(q_1), Self::ExactOut(q_2)) => Self::ExactIn(q_1 - q_2),
            (Self::ExactOut(q_1), Self::ExactIn(q_2)) if q_2 > q_1 => Self::ExactIn(q_2 - q_1),
            (Self::ExactOut(q_1), Self::ExactIn(q_2)) => Self::ExactOut(q_1 - q_2)
        }
    }
}

impl Sub<u128> for DebtType {
    type Output = Self;

    fn sub(self, rhs: u128) -> Self::Output {
        match self {
            Self::ExactIn(q) => Self::ExactIn(q.saturating_sub(rhs)),
            Self::ExactOut(q) => Self::ExactOut(q.saturating_sub(rhs))
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct Debt {
    cur_price: Ray,
    magnitude: DebtType
}

impl Debt {
    pub fn new<T: Into<Ray>>(magnitude: DebtType, price: T) -> Self {
        let cur_price: Ray = price.into();
        Self { cur_price, magnitude }
    }

    /// Creates a new Debt item at the price provided with the same quantity as
    /// the current Debt
    pub fn set_price(&self, new_price: Ray) -> Self {
        Self { cur_price: new_price, ..*self }
    }

    pub fn magnitude(&self) -> u128 {
        self.magnitude.magnitude()
    }

    pub fn price(&self) -> Ray {
        self.cur_price
    }

    /// Returns the additional T0 needed to increase a debt from a specific T1
    /// to another greater T1 count
    pub fn additional_t0_needed(&self, added_t1: u128) -> u128 {
        let new_magnitude = self.magnitude + self.magnitude.same_type(added_t1);
        let new_debt = Self { magnitude: new_magnitude, cur_price: self.cur_price };
        new_debt.current_t0() - self.current_t0()
    }

    /// Does this debt exist on the bid side of the book?  A debt is on the bid
    /// side of the book when it's an ExactIn debt (i.e. I would like to pay 100
    /// T1 and get as many T0 as I can for that amount)
    pub fn bid_side(&self) -> bool {
        matches!(self.magnitude, DebtType::ExactIn(_))
    }

    /// Return the T1 quantity of debt "slack" - i.e. the amount that is being
    /// rounded away at the current price.  This is primarily for use in order
    /// quantity calculations to ensure we don't over or underfill orders
    pub fn slack(&self) -> u128 {
        self.magnitude.slack_at_price(self.cur_price)
    }

    /// Given the Debt's direction and rounding, return the low and high price
    /// that will result in an exchange for an identical amount of T0
    pub fn price_range(&self) -> (Ray, Ray) {
        let current_amount = self.current_t0();
        let bound_amount = match self.magnitude {
            DebtType::ExactIn(_) => current_amount + 1,
            DebtType::ExactOut(_) => current_amount.saturating_sub(1)
        };
        let (current_price, bound_price) = match (current_amount, bound_amount) {
            // If both values are zero, something is hecka wrong
            (0, 0) => (Ray::min_uniswap_price(), Ray::max_uniswap_price()),
            // If either value is 0, one of our bounds is max price
            (0, p) | (p, 0) => {
                (Ray::max_uniswap_price(), Ray::calc_price_generic(p, self.magnitude()))
            }
            // Otherwise we can just do our normal math
            (c, b) => (
                Ray::calc_price_generic(c, self.magnitude()),
                Ray::calc_price_generic(b, self.magnitude())
            )
        };
        let (low, high) = low_to_high(&current_price, &bound_price);
        (*low, *high)
    }

    /// Will return true if the price provided is equivalent to the current
    /// price of this debt due to the rounding innate to performing the actual
    /// transaction
    pub fn valid_for_price(&self, price: Ray) -> bool {
        let (low, high) = self.price_range();

        match self.magnitude {
            DebtType::ExactIn(_) => price > low && price <= high,
            DebtType::ExactOut(_) => price >= low && price < high
        }
    }

    pub fn current_t0(&self) -> u128 {
        self.magnitude.t0_at_price(self.cur_price)
    }

    pub fn freed_t0(&self, t1_change: u128) -> u128 {
        let i_t0 = self.current_t0();
        let f_t0 = self
            .magnitude
            .same_type(self.magnitude.magnitude().saturating_sub(t1_change))
            .t0_at_price(self.cur_price);
        i_t0.saturating_sub(f_t0)
    }

    /// Create a new Debt object based on the price change created by filling
    /// the debt with a specified amount of t0
    pub fn partial_fill(&self, q: u128) -> Self {
        // P' = P + (y/q) = self.cur_price + Ray::calc_price(y, q)
        let price_diff = Ray::calc_price_generic(self.magnitude(), q);
        let new_price = match self.magnitude {
            DebtType::ExactIn(_) => self.cur_price + price_diff,
            DebtType::ExactOut(_) => self.cur_price - price_diff
        };
        Self { magnitude: self.magnitude, cur_price: new_price }
    }

    /// Create a new Debt object based on the price change created by filling
    /// the debt with a specified amount of T1
    pub fn partial_fill_t1(&self, q: u128) -> Self {
        let current_t0 = self.current_t0();
        let new_t1 = self.magnitude - q;
        let new_price = Ray::calc_price_generic(current_t0, new_t1.magnitude());
        Self { magnitude: new_t1, cur_price: new_price }
    }

    /// Create a new debt object which just has our T1 reduced by the specified
    /// amount, the intention being that the T0 previously allocated has been
    /// elsewise pushed to a paired AMM in a Composite order
    pub fn flat_fill_t1(&self, q: u128) -> Self {
        let new_t1 = self.magnitude - q;
        Self { magnitude: new_t1, cur_price: self.cur_price }
    }

    /// Difference in t0 required to move the price from the current price to
    /// the target price
    pub fn dq_to_price(&self, target_price: &Ray) -> u128 {
        let final_t0 = self.magnitude.t0_at_price(*target_price);
        let current_t0: u128 = self.current_t0();
        if final_t0 > current_t0 {
            final_t0 - current_t0
        } else {
            current_t0 - final_t0
        }
    }

    /// Given an AMM liquidity and a quantity of t0 being bought to or sold from
    /// the AMM, returns the quantity that t0 will change to bring the price
    /// of this debt to the same price as the AMM
    pub fn calc_proportion(
        &self,
        amm_delta: u128,
        amm_liquidity: u128,
        amm_positive_delta: bool
    ) -> u128 {
        // Put our constants into Integer format
        let t1 = Natural::from(self.magnitude.magnitude());
        let t0_start = Natural::from(self.current_t0());
        let l = Natural::from(amm_liquidity);
        let dx = Natural::from(amm_delta);

        // Find our Sqrts that we're using, with extra precision baked in
        let sqrt_t1_x96 = ((&t1 << 192) as Natural).floor_sqrt();
        let sqrt_t0_start_x96 = ((&t0_start << 192) as Natural).floor_sqrt();

        /* - Very succinct but the order depends on what we're doing
        let numerator = (Natural::from(amm_delta) * &sqrt_t1_x96)
            .add_mul(Natural::from(amm_liquidity), &sqrt_t0_start_x96);
        */

        let a_num_portion_1 = &dx * &sqrt_t1_x96;
        println!(
            "----- Num portion 1\nRaw: {}\nReduced: {}\n-----",
            a_num_portion_1,
            &a_num_portion_1 >> 96
        );

        let a_num_portion_2 = &l * &sqrt_t0_start_x96;
        println!(
            "----- Num portion 2\nRaw: {}\nReduced: {}\n-----",
            a_num_portion_2,
            &a_num_portion_2 >> 96
        );
        let a_numerator_sum = if amm_positive_delta {
            a_num_portion_1 + a_num_portion_2
        } else {
            a_num_portion_2 - a_num_portion_1
        };

        // let a_numerator_sum = a_num_portion_2 - a_num_portion_1;
        println!(
            "----- Num sum\nRaw: {}\nReduced: {}\n-----",
            a_numerator_sum,
            &a_numerator_sum >> 96
        );

        let (a_fraction, _) = (&a_numerator_sum).div_round(&l, RoundingMode::Nearest);
        println!(
            "--- Fraction calc ---\nNumerator: {}\nDenominator: {}\nResult: {}\nRounded result: \
             {}\n--------------------",
            a_numerator_sum,
            &l,
            a_fraction,
            &a_fraction >> 96
        );

        // if A = sqrt(x + dX) then we have to square A and subtract the original X
        let debt_delta_t0 = &t0_start
            - &a_fraction
                .pow(2)
                .div_round(const_2_192(), RoundingMode::Ceiling)
                .0;
        u128::saturating_from(&debt_delta_t0)
    }
}

impl Add<Debt> for Option<Debt> {
    type Output = Option<Debt>;

    fn add(self, rhs: Debt) -> Self::Output {
        match self {
            None => Some(rhs),
            Some(d) => d + rhs
        }
    }
}

impl AddAssign<Debt> for Option<Debt> {
    fn add_assign(&mut self, rhs: Debt) {
        *self = *self + rhs
    }
}

impl Add<Debt> for Debt {
    type Output = Option<Debt>;

    fn add(self, rhs: Debt) -> Self::Output {
        let magnitude = self.magnitude + rhs.magnitude;
        if magnitude.magnitude() == 0 {
            return None
        }
        // If our new magnitude is on the same side, we stay at our price.  If we flip,
        // we flip to the other price
        let cur_price =
            if magnitude.same_side(&self.magnitude) { self.cur_price } else { rhs.cur_price };
        Some(Self { magnitude, cur_price })
    }
}

impl<'a> Add<DebtType> for &'a Debt {
    type Output = Debt;

    fn add(self, rhs: DebtType) -> Self::Output {
        Debt { cur_price: self.cur_price, magnitude: self.magnitude + rhs }
    }
}

impl<'a> PartialEq<PoolPrice<'a>> for Debt {
    fn eq(&self, other: &PoolPrice<'a>) -> bool {
        self.valid_for_price(other.as_ray())
    }
}

impl<'a> PartialOrd<PoolPrice<'a>> for Debt {
    fn partial_cmp(&self, other: &PoolPrice<'a>) -> Option<std::cmp::Ordering> {
        if self == other {
            Some(std::cmp::Ordering::Equal)
        } else {
            Some(self.price().cmp(&other.as_ray()))
        }
    }
}
